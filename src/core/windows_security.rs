//! Windows identities, hash pins and fail-closed path checks for the action boundary.
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Reject every reparse point, including junctions in ancestor directories.
/// ACLs must additionally prevent untrusted users swapping checked directories.
pub fn safe_existing_path(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        bail!("expected absolute path without parent components");
    }
    let mut prefix = PathBuf::new();
    for component in path.components() {
        prefix.push(component.as_os_str());
        if matches!(component, Component::Prefix(_) | Component::RootDir) {
            continue;
        }
        let metadata = fs::symlink_metadata(&prefix).context("inspect trusted path component")?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                bail!("reparse point in action path");
            }
        }
        if metadata.file_type().is_symlink() {
            bail!("symlink in action path");
        }
    }
    fs::canonicalize(path).context("canonicalize safe action path")
}

#[cfg(windows)]
pub fn sha256(bytes: &[u8]) -> Result<String> {
    use windows_sys::Win32::Security::Cryptography::*;
    if bytes.len() > u32::MAX as usize {
        bail!("hash input too large");
    }
    let mut digest = [0u8; 32];
    unsafe {
        let mut algorithm = std::ptr::null_mut();
        if BCryptOpenAlgorithmProvider(&mut algorithm, BCRYPT_SHA256_ALGORITHM, std::ptr::null(), 0)
            < 0
        {
            bail!("SHA256 provider unavailable");
        }
        let status = BCryptHash(
            algorithm,
            std::ptr::null(),
            0,
            bytes.as_ptr(),
            bytes.len() as u32,
            digest.as_mut_ptr(),
            32,
        );
        BCryptCloseAlgorithmProvider(algorithm, 0);
        if status < 0 {
            bail!("SHA256 failed");
        }
    }
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(not(windows))]
pub fn sha256(_bytes: &[u8]) -> Result<String> {
    bail!("action hashing requires Windows")
}

pub fn file_sha256(path: &Path) -> Result<String> {
    let path = safe_existing_path(path)?;
    if !path.is_file() || fs::metadata(&path)?.len() > 128 * 1024 * 1024 {
        bail!("hash pin requires regular executable <=128MiB");
    }
    sha256(&fs::read(path)?)
}

#[derive(Debug, serde::Serialize)]
pub struct ProcessIdentity {
    pub sid: String,
    pub privileged: bool,
}

#[cfg(windows)]
pub fn process_identity() -> Result<ProcessIdentity> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::*;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            bail!("open process token");
        }
        let result = (|| -> Result<ProcessIdentity> {
            let mut needed = 0;
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
            let mut buffer = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
            if GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            ) == 0
            {
                bail!("read token user");
            }
            let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
            let mut sid_string = std::ptr::null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut sid_string) == 0 {
                bail!("convert token SID");
            }
            let mut length = 0;
            while *sid_string.add(length) != 0 {
                length += 1;
            }
            let sid = String::from_utf16_lossy(std::slice::from_raw_parts(sid_string, length));
            LocalFree(sid_string.cast());
            let mut elevation: TOKEN_ELEVATION = zeroed();
            if GetTokenInformation(
                token,
                TokenElevation,
                (&mut elevation as *mut TOKEN_ELEVATION).cast(),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut needed,
            ) == 0
            {
                bail!("read token elevation");
            }
            // Reject even deny-only Administrators membership: a filtered admin
            // token is not a dedicated non-admin service identity.
            GetTokenInformation(token, TokenGroups, std::ptr::null_mut(), 0, &mut needed);
            let mut groups_buffer = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
            if GetTokenInformation(
                token,
                TokenGroups,
                groups_buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            ) == 0
            {
                bail!("read token groups");
            }
            let groups = &*groups_buffer.as_ptr().cast::<TOKEN_GROUPS>();
            let admin =
                std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize)
                    .iter()
                    .any(|group| IsWellKnownSid(group.Sid, WinBuiltinAdministratorsSid) != 0);
            let forbidden = ["S-1-5-18", "S-1-5-19", "S-1-5-20"].contains(&sid.as_str());
            GetTokenInformation(token, TokenPrivileges, std::ptr::null_mut(), 0, &mut needed);
            let mut privileges_buffer =
                vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
            if GetTokenInformation(
                token,
                TokenPrivileges,
                privileges_buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            ) == 0
            {
                bail!("read token privileges");
            }
            let privileges = &*privileges_buffer.as_ptr().cast::<TOKEN_PRIVILEGES>();
            let privileges = std::slice::from_raw_parts(
                privileges.Privileges.as_ptr(),
                privileges.PrivilegeCount as usize,
            );
            let mut dangerous = false;
            for name in [
                "SeBackupPrivilege",
                "SeRestorePrivilege",
                "SeDebugPrivilege",
                "SeTcbPrivilege",
                "SeAssignPrimaryTokenPrivilege",
                "SeImpersonatePrivilege",
                "SeLoadDriverPrivilege",
                "SeTakeOwnershipPrivilege",
                "SeCreateTokenPrivilege",
                "SeSecurityPrivilege",
                "SeManageVolumePrivilege",
            ] {
                let wide = name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
                let mut luid = zeroed();
                if LookupPrivilegeValueW(std::ptr::null(), wide.as_ptr(), &mut luid) == 0 {
                    bail!("resolve Windows privilege");
                }
                // Disabled privileges can still be enabled by the process.
                dangerous |= privileges.iter().any(|privilege| {
                    privilege.Luid.LowPart == luid.LowPart
                        && privilege.Luid.HighPart == luid.HighPart
                        && privilege.Attributes & SE_PRIVILEGE_REMOVED == 0
                });
            }
            Ok(ProcessIdentity {
                sid,
                privileged: elevation.TokenIsElevated != 0 || admin || forbidden || dangerous,
            })
        })();
        CloseHandle(token);
        result
    }
}

#[cfg(not(windows))]
pub fn process_identity() -> Result<ProcessIdentity> {
    bail!("action runner requires Windows")
}

pub fn require_runner_identity(expected: &str, daemon_sid: &str) -> Result<ProcessIdentity> {
    let identity = process_identity()?;
    if identity.sid != expected || identity.sid == daemon_sid || identity.privileged {
        bail!("runner must use the configured distinct dedicated non-admin Windows SID");
    }
    Ok(identity)
}
