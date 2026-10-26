param(
    [Parameter(Mandatory)][string]$BinaryDirectory,
    [Parameter(Mandatory)][string]$ArtifactManifest,
    [Parameter(Mandatory)][string]$BackupRoot,
    [Parameter(Mandatory)][string]$RehearsalRoot,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{40}$')][string]$Commit
)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$productionRoot='C:\ProgramData\coord\g7'
$backupPath=[IO.Path]::GetFullPath($BackupRoot)
$base=[IO.Path]::GetFullPath($RehearsalRoot)
if([IO.Path]::GetDirectoryName($base)-ine $backupPath -or [IO.Path]::GetFileName($base)-notmatch '^r5-1b-[a-f0-9]{8}$'){throw 'Rehearsal root must be a fresh, exact child of the protected G7 backup.'}
$siteId=[IO.Path]::GetFileName($base).Substring(6)
$userName='r5b1b_'+$siteId
$runnerService='CoordR5B1BRunner'+$siteId
$probeService='CoordR5B1BCore'+$siteId
$productionRunnerService='CoordActionRunnerG7'
$servy='C:\Program Files\Servy\servy-cli.exe'
$daemonSid='S-1-5-19'
if(-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Administrator operator required.'}
if(Test-Path -LiteralPath $base){throw 'Rehearsal root already exists; inspect its owned state instead of overwriting.'}
if(Get-LocalUser -Name $userName -ErrorAction SilentlyContinue){throw 'Runner account already exists; refusing to repurpose it.'}
foreach($name in @($runnerService,$probeService)){if(Get-Service -Name $name -ErrorAction SilentlyContinue){throw 'Candidate service name already exists.'}}
foreach($port in @(17777,17779)){if(Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue){throw 'Candidate loopback port is occupied.'}}
$backup=Get-Content -Raw -LiteralPath (Join-Path $BackupRoot 'backup-manifest.json')|ConvertFrom-Json
if($backup.state -ne 'PASS' -or $backup.installed_binary_sha256 -ne (Get-FileHash 'C:\ProgramData\coord\bin\coord.exe').Hash.ToLowerInvariant()){throw 'Accepted backup/installed binary mismatch.'}
$productionRunner=Get-CimInstance Win32_Service -Filter "Name='$productionRunnerService'"
$productionRunnerBackup=Get-Content -Raw -LiteralPath (Join-Path $BackupRoot ('services/'+$productionRunnerService+'.json'))|ConvertFrom-Json
$productionRunnerPath=[IO.Path]::GetFullPath('C:\ProgramData\coord\g7\bin\coord-action-runner.exe')
if(-not $productionRunner -or $productionRunner.StartName -ne $productionRunnerBackup.StartName -or [IO.Path]::GetFullPath([string]$productionRunnerBackup.ExecutablePath) -ine $productionRunnerPath -or $productionRunner.PathName -notmatch [regex]::Escape($productionRunnerPath)){throw 'Current production Runner identity/path does not match the protected service backup.'}
$productionRunnerSid=([Security.Principal.NTAccount]::new([string]$productionRunner.StartName)).Translate([Security.Principal.SecurityIdentifier]).Value
if($productionRunnerSid -in @('S-1-5-18','S-1-5-19','S-1-5-20')){throw 'Production Runner must retain its dedicated low-privilege identity.'}
$manifest=Get-Content -Raw -LiteralPath $ArtifactManifest|ConvertFrom-Json
if($manifest.state -ne 'PASS' -or $manifest.commit -ne $Commit -or @($manifest.files).Count -ne 3){throw 'Matched verified CI artifact required.'}
foreach($name in @('coord.exe','coord-action-runner.exe','coord-emote-action.exe')){
 $entry=@($manifest.files|Where-Object name -eq $name)
 if($entry.Count -ne 1 -or (Get-FileHash -LiteralPath (Join-Path $BinaryDirectory $name)).Hash.ToLowerInvariant() -ne $entry[0].sha256){throw 'Candidate executable hash mismatch.'}
}
$healthMonitorEntry=$manifest.health_monitor_script
$artifactRoot=[IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($ArtifactManifest))
$healthMonitorArtifact=Join-Path $artifactRoot 'coord-health-monitor.ps1'
if(-not $healthMonitorEntry -or $healthMonitorEntry.name -ne 'coord-health-monitor.ps1' -or -not(Test-Path -LiteralPath $healthMonitorArtifact -PathType Leaf) -or (Get-FileHash -LiteralPath $healthMonitorArtifact -Algorithm SHA256).Hash.ToLowerInvariant() -ne [string]$healthMonitorEntry.sha256){throw 'Matched CI health-monitor script and hash are required.'}
if($manifest.platform-ne 'windows-2022-x86_64-pc-windows-msvc' -or $manifest.zip.name-ne 'coord-r5-1b-windows-2022.zip'){throw 'Expected Windows R5-1B package metadata.'}
$artifactZip=Join-Path $artifactRoot ([string]$manifest.zip.name)
if(-not(Test-Path -LiteralPath $artifactZip -PathType Leaf) -or (Get-FileHash -LiteralPath $artifactZip -Algorithm SHA256).Hash.ToLowerInvariant()-ne [string]$manifest.zip.sha256){throw 'R5-1B ZIP does not match the CI manifest.'}
function Set-SiteAcl([string]$Path){
 & icacls.exe $Path /inheritance:r|Out-Null
 if($LASTEXITCODE -ne 0){throw 'Exact candidate ACL failed.'}
 Add-SiteAce $Path '*S-1-5-18:(OI)(CI)F'
 Add-SiteAce $Path '*S-1-5-32-544:(OI)(CI)F'
}
function Add-SiteAce([string]$Path,[string]$Ace){
 & icacls.exe $Path /grant:r $Ace|Out-Null
 if($LASTEXITCODE -ne 0){throw 'Exact candidate ACL failed.'}
}
function Servy([string[]]$Arguments,[string]$Label){
 & $servy @Arguments *> (Join-Path $base ('private/'+$Label+'.log'))
 if($LASTEXITCODE -ne 0){throw ('Candidate service configuration failed: '+$Label)}
}
New-Item -ItemType Directory -Path $base|Out-Null
Set-SiteAcl $base
foreach($relative in @('bin','config','broker-public','runner-private','test-broker-public','test-runner-private','private','daemon-private','monitor-private','relay-private','verification','runtime/logs','runtime/scratch','probe/business/refs/arknights/icons','probe/business/task/00','probe/business/results/.receipts','probe/business/results/review_queue','canary/business/refs/arknights/icons','canary/business/results/.receipts','canary/business/results/review_queue')){New-Item -ItemType Directory -Path (Join-Path $base $relative) -Force|Out-Null}
$passwordBytes=New-Object byte[] 48
$randomGenerator=[Security.Cryptography.RandomNumberGenerator]::Create()
try{$randomGenerator.GetBytes($passwordBytes)}finally{$randomGenerator.Dispose()}
$passwordText=[Convert]::ToBase64String($passwordBytes)+'aA1!'
$password=ConvertTo-SecureString $passwordText -AsPlainText -Force
$user=New-LocalUser -Name $userName -Password $password -AccountNeverExpires -PasswordNeverExpires -Description 'Dedicated coord G7 completion runner'
Add-LocalGroupMember -SID 'S-1-5-32-545' -Member $userName
$runnerSid=$user.SID.Value
$runnerRx='*'+$runnerSid+':(OI)(CI)RX';$runnerModify='*'+$runnerSid+':(OI)(CI)M';$daemonRx='*S-1-5-19:(OI)(CI)RX'
foreach($relative in @('runner-private','test-runner-private')){Set-SiteAcl (Join-Path $base $relative);Add-SiteAce (Join-Path $base $relative) $runnerRx}
foreach($relative in @('broker-public','test-broker-public')){Set-SiteAcl (Join-Path $base $relative);Add-SiteAce (Join-Path $base $relative) $runnerRx;Add-SiteAce (Join-Path $base $relative) $daemonRx}
$credential=[Management.Automation.PSCredential]::new(($env:COMPUTERNAME+'\'+$userName),$password)
$credential|Export-Clixml -LiteralPath (Join-Path $base 'private/runner.credential.clixml')
$passwordText=$null
@{state='ACCOUNT_CREATED';commit=$Commit;base=$base;backup_root=$BackupRoot;runner_sid=$runnerSid;daemon_sid=$daemonSid;runner_service=$runnerService;probe_service=$probeService;production_replacement='NOT_RUN'}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $base 'private/site-state.json') -Encoding utf8NoBOM
foreach($name in @('coord.exe','coord-action-runner.exe','coord-emote-action.exe')){Copy-Item -LiteralPath (Join-Path $BinaryDirectory $name) -Destination (Join-Path $base ('bin/'+$name))}
Copy-Item -LiteralPath (Join-Path $env:SystemRoot 'System32/ping.exe') -Destination (Join-Path $base 'bin/ping-fixture.exe')
Copy-Item -LiteralPath (Join-Path $BackupRoot 'coord.db') -Destination (Join-Path $base 'verification/state.db')
foreach($relative in @('daemon-private/management.key','relay-private/identity.key')){$keyBytes=New-Object byte[] 32;$randomGenerator=[Security.Cryptography.RandomNumberGenerator]::Create();try{$randomGenerator.GetBytes($keyBytes)}finally{$randomGenerator.Dispose()};$keyValue=([BitConverter]::ToString($keyBytes)-replace '-','').ToLowerInvariant();[IO.File]::WriteAllText((Join-Path $base $relative),$keyValue)}
$privateKeyPath=Join-Path $base 'test-runner-private/runner-test.pk8'
$publicKeyPath=Join-Path $base 'test-broker-public/runner-test-public.key'
& (Join-Path $base 'bin/coord-action-runner.exe') --generate-keypair --private-key-file $privateKeyPath --public-key-file $publicKeyPath *> (Join-Path $base 'runtime/logs/runner-keygen-test.log')
if($LASTEXITCODE -ne 0){throw 'Isolated Ed25519 Runner key generation failed.'}
$runnerKeyId='g7-test-'+[guid]::NewGuid().ToString('N')
Copy-Item -LiteralPath (Join-Path $base 'daemon-private/management.key') -Destination (Join-Path $base 'monitor-private/management.key')
$image=[Convert]::FromBase64String('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+c9i8AAAAASUVORK5CYII=')
foreach($business in @('probe/business','canary/business')){
 [IO.File]::WriteAllBytes((Join-Path $base ($business+'/refs/arknights/icons/marker.png')),$image)
 [IO.File]::WriteAllText((Join-Path $base ($business+'/refs/operators.csv')),"name,game`nSynthetic marker,unknown`n")
 foreach($i in 0..11){
  $group=$i.ToString('00');$worker=$i.ToString('0000')
  New-Item -ItemType Directory -Path (Join-Path $base ($business+'/task/'+$group)) -Force|Out-Null
  [IO.File]::WriteAllBytes((Join-Path $base ($business+'/task/'+$group+'/'+($i+1)+'.png')),$image)
  if($i -le 5){
   $workspace=Join-Path $base ($business+'/workers/'+$worker+'/attempts')
   New-Item -ItemType Directory -Path $workspace -Force|Out-Null
   [IO.File]::WriteAllText((Join-Path $workspace 'private.json'),'{}')
  }
 }
}
$publisher=Join-Path $base 'bin/coord-emote-action.exe'
$scratch=Join-Path $base 'runtime/scratch'
$profiles=@{}
foreach($kind in @('probe','canary')){
 $command=@{executable=$publisher;sha256=(Get-FileHash $publisher).Hash.ToLowerInvariant();working_directory=$scratch;args=@('--root',(Join-Path $base ($kind+'/business')));include_result=$true;timeout_ms=10000;max_output_bytes=65536;max_attempts=2;idempotent=$true}
 $profiles['g7-'+$kind+'-v1']=@{version=1;on_task_completed=$command;on_batch_completed=$command}
}
$profiles['g7-crash-probe-v1']=@{version=1;on_task_completed=@{executable=(Join-Path $base 'bin/ping-fixture.exe');sha256=(Get-FileHash (Join-Path $base 'bin/ping-fixture.exe')).Hash.ToLowerInvariant();working_directory=$scratch;args=@('-n','30','127.0.0.1');include_result=$false;timeout_ms=30000;max_output_bytes=65536;max_attempts=1;idempotent=$false}}
$registry=Join-Path $base 'config/registry-probe.json'
$managementKeyPath=Join-Path $base 'daemon-private/management.key'
@{version=1;allowed_root=$base;management_rpc_key_file=$managementKeyPath;external_runner=@{broker_addr='127.0.0.1:17779';runner_sid=$runnerSid;daemon_sid=$daemonSid;runner_key_id=$runnerKeyId;runner_public_key_file=$publicKeyPath};profiles=$profiles}|ConvertTo-Json -Depth 25|Set-Content -LiteralPath $registry -Encoding utf8NoBOM
Set-SiteAcl $base
Add-SiteAce $base $runnerRx
Add-SiteAce $base $daemonRx
foreach($relative in @('private','monitor-private')){Set-SiteAcl (Join-Path $base $relative)}
foreach($relative in @('daemon-private','relay-private')){Set-SiteAcl (Join-Path $base $relative);Add-SiteAce (Join-Path $base $relative) $daemonRx}
Set-SiteAcl (Join-Path $base 'verification')
Add-SiteAce (Join-Path $base 'verification') '*S-1-5-19:(OI)(CI)M'
foreach($relative in @('runtime/logs','runtime/scratch','probe/business/results','canary/business/results')){Set-SiteAcl (Join-Path $base $relative);Add-SiteAce (Join-Path $base $relative) $runnerModify;Add-SiteAce (Join-Path $base $relative) '*S-1-5-19:(OI)(CI)M'}
foreach($relative in @('probe/business/workers','canary/business/workers')){Set-SiteAcl (Join-Path $base $relative)}
$probe=@{read_files=@($registry,$privateKeyPath,$publicKeyPath,$publisher,(Join-Path $base 'probe/business/task/00/1.png'),(Join-Path $base 'probe/business/refs/arknights/icons/marker.png'));denied_read_files=@('C:\ProgramData\coord\data\coord.db',(Join-Path $base 'verification/state.db'),(Join-Path $base 'daemon-private/management.key'),(Join-Path $base 'monitor-private/management.key'),(Join-Path $base 'relay-private/identity.key'))+@(0..5|ForEach-Object{Join-Path $base ('probe/business/workers/'+$_.ToString('0000')+'/attempts/private.json')});denied_write_files=@($registry,$publisher,(Join-Path $base 'bin/coord.exe'));write_directories=@($scratch,(Join-Path $base 'runtime/logs'),(Join-Path $base 'probe/business/results'));denied_write_directories=@((Join-Path $base 'verification'),(Join-Path $base 'config'),(Join-Path $base 'broker-public'),(Join-Path $base 'runner-private'),(Join-Path $base 'test-broker-public'),(Join-Path $base 'test-runner-private'),(Join-Path $base 'bin'),$env:SystemRoot);denied_rpc_url='http://127.0.0.1:17777/'}
$probe|ConvertTo-Json -Depth 10|Set-Content -LiteralPath (Join-Path $base 'config/acl-probe.json') -Encoding utf8NoBOM
Servy -Arguments @('install','--name',$probeService,'--path',(Join-Path $base 'bin/coord.exe'),'--startupDir',$base,'--params',('serve --addr 127.0.0.1:17777 --db "'+(Join-Path $base 'verification/state.db')+'" --actions-config "'+$registry+'"'),'--user','NT AUTHORITY\LocalService','--startupType','Manual','--stdout',(Join-Path $base 'runtime/logs/probe-daemon.stdout.log'),'--stderr',(Join-Path $base 'runtime/logs/probe-daemon.stderr.log'),'--quiet') -Label 'probe-service-install'
$previousPassword=[Environment]::GetEnvironmentVariable('SERVY_PASSWORD','Process')
try{
 [Environment]::SetEnvironmentVariable('SERVY_PASSWORD',$credential.GetNetworkCredential().Password,'Process')
 Servy -Arguments @('install','--name',$runnerService,'--path',(Join-Path $base 'bin/coord-action-runner.exe'),'--startupDir',$base,('--params=--registry "'+$registry+'" --private-key-file "'+$privateKeyPath+'"'),'--user',($env:COMPUTERNAME+'\'+$userName),'--startupType','Manual','--stdout',(Join-Path $base 'runtime/logs/runner.stdout.log'),'--stderr',(Join-Path $base 'runtime/logs/runner.stderr.log'),'--quiet') -Label 'runner-service-install'
}finally{[Environment]::SetEnvironmentVariable('SERVY_PASSWORD',$previousPassword,'Process')}
& sc.exe privs $runnerService SeChangeNotifyPrivilege *> (Join-Path $base 'private/runner-required-privileges.log')
if($LASTEXITCODE -ne 0){throw 'Runner SCM minimum-privilege configuration failed.'}
$state=@{state='PREPARED';commit=$Commit;base=$base;backup_root=$BackupRoot;site_id=$siteId;runner_sid=$runnerSid;runner_account=$userName;daemon_sid=$daemonSid;runner_service=$runnerService;probe_service=$probeService;system_health_task_name=('coord-r5-1b-health-'+$siteId);registry=$registry;test_runner_key_id=$runnerKeyId;test_runner_private_key_path=$privateKeyPath;test_runner_public_key_path=$publicKeyPath;production_runner_service=$productionRunnerService;production_runner_sid=$productionRunnerSid;production_runner_executable_path=$productionRunnerPath;production_runner_private_key_directory=(Join-Path $productionRoot 'runner-private');production_runner_public_key_directory=(Join-Path $productionRoot 'broker-public');probe_url='http://127.0.0.1:17777/';credential_storage='Isolated test account; SCM plus administrator-only DPAPI credential; transient installer environment cleared';artifact_files=$manifest.files;artifact_zip_sha256=[string]$manifest.zip.sha256;artifact_health_monitor_sha256=[string]$healthMonitorEntry.sha256;production_replacement='NOT_RUN'}
$state|ConvertTo-Json -Depth 10|Set-Content -LiteralPath (Join-Path $base 'private/site-state.json') -Encoding utf8NoBOM
$state|ConvertTo-Json -Depth 10
