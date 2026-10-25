param(
    [Parameter(Mandatory)][string]$BinaryDirectory,
    [Parameter(Mandatory)][string]$ArtifactManifest,
    [Parameter(Mandatory)][string]$BackupRoot,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{40}$')][string]$Commit
)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$base='C:\ProgramData\coord\g7'
$userName='coord_action_g7'
$runnerService='CoordActionRunnerG7'
$probeService='CoordG7Probe'
$servy='C:\Program Files\Servy\servy-cli.exe'
$daemonSid='S-1-5-19'
if(-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Administrator operator required.'}
if(Test-Path -LiteralPath $base){throw 'G7 site already exists; inspect its owned state instead of overwriting.'}
if(Get-LocalUser -Name $userName -ErrorAction SilentlyContinue){throw 'Runner account already exists; refusing to repurpose it.'}
foreach($name in @($runnerService,$probeService)){if(Get-Service -Name $name -ErrorAction SilentlyContinue){throw 'Candidate service name already exists.'}}
foreach($port in @(17777,17779)){if(Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue){throw 'Candidate loopback port is occupied.'}}
$backup=Get-Content -Raw -LiteralPath (Join-Path $BackupRoot 'backup-manifest.json')|ConvertFrom-Json
if($backup.state -ne 'PASS' -or $backup.installed_binary_sha256 -ne (Get-FileHash 'C:\ProgramData\coord\bin\coord.exe').Hash.ToLowerInvariant()){throw 'Accepted backup/installed binary mismatch.'}
$manifest=Get-Content -Raw -LiteralPath $ArtifactManifest|ConvertFrom-Json
if($manifest.state -ne 'PASS' -or $manifest.commit -ne $Commit -or @($manifest.files).Count -ne 3){throw 'Matched verified CI artifact required.'}
foreach($name in @('coord.exe','coord-action-runner.exe','coord-emote-action.exe')){
 $entry=@($manifest.files|Where-Object name -eq $name)
 if($entry.Count -ne 1 -or (Get-FileHash -LiteralPath (Join-Path $BinaryDirectory $name)).Hash.ToLowerInvariant() -ne $entry[0].sha256){throw 'Candidate executable hash mismatch.'}
}
function Set-SiteAcl([string]$Path,[string[]]$Extra=@()){
 & icacls.exe $Path /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' @Extra|Out-Null
 if($LASTEXITCODE -ne 0){throw 'Exact candidate ACL failed.'}
}
function Servy([string[]]$Arguments,[string]$Label){
 & $servy @Arguments *> (Join-Path $base ('private/'+$Label+'.log'))
 if($LASTEXITCODE -ne 0){throw ('Candidate service configuration failed: '+$Label)}
}
New-Item -ItemType Directory -Path $base|Out-Null
Set-SiteAcl $base
foreach($relative in @('bin','config','private','daemon-private','monitor-private','relay-private','verification','runtime/logs','runtime/scratch','probe/business/refs/arknights/icons','probe/business/task/00','probe/business/results/.receipts','probe/business/results/review_queue','canary/business/refs/arknights/icons','canary/business/results/.receipts','canary/business/results/review_queue')){New-Item -ItemType Directory -Path (Join-Path $base $relative) -Force|Out-Null}
$passwordText=[Convert]::ToBase64String([Security.Cryptography.RandomNumberGenerator]::GetBytes(48))+'aA1!'
$password=ConvertTo-SecureString $passwordText -AsPlainText -Force
$user=New-LocalUser -Name $userName -Password $password -AccountNeverExpires -PasswordNeverExpires -Description 'Dedicated coord G7 completion runner'
Add-LocalGroupMember -SID 'S-1-5-32-545' -Member $userName
$runnerSid=$user.SID.Value
$credential=[Management.Automation.PSCredential]::new(($env:COMPUTERNAME+'\'+$userName),$password)
$credential|Export-Clixml -LiteralPath (Join-Path $base 'private/runner.credential.clixml')
$passwordText=$null
@{state='ACCOUNT_CREATED';commit=$Commit;base=$base;backup_root=$BackupRoot;runner_sid=$runnerSid;daemon_sid=$daemonSid;runner_service=$runnerService;probe_service=$probeService;production_replacement='NOT_RUN'}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $base 'private/site-state.json') -Encoding utf8NoBOM
foreach($name in @('coord.exe','coord-action-runner.exe','coord-emote-action.exe')){Copy-Item -LiteralPath (Join-Path $BinaryDirectory $name) -Destination (Join-Path $base ('bin/'+$name))}
Copy-Item -LiteralPath (Join-Path $env:SystemRoot 'System32/ping.exe') -Destination (Join-Path $base 'bin/ping-fixture.exe')
Copy-Item -LiteralPath (Join-Path $BackupRoot 'coord.db') -Destination (Join-Path $base 'verification/state.db')
foreach($relative in @('config/broker.key','daemon-private/management.key','relay-private/identity.key')){[IO.File]::WriteAllText((Join-Path $base $relative),[Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32)).ToLowerInvariant())}
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
@{version=1;allowed_root=$base;external_runner=@{broker_addr='127.0.0.1:17779';key_file=(Join-Path $base 'config/broker.key');daemon_rpc_key_file=(Join-Path $base 'daemon-private/management.key');runner_sid=$runnerSid;daemon_sid=$daemonSid};profiles=$profiles}|ConvertTo-Json -Depth 25|Set-Content -LiteralPath $registry -Encoding utf8NoBOM
$runnerRx='*'+$runnerSid+':(OI)(CI)RX';$runnerModify='*'+$runnerSid+':(OI)(CI)M';$daemonRx='*S-1-5-19:(OI)(CI)RX'
& icacls.exe 'C:\ProgramData\coord' /grant ('*'+$runnerSid+':(RX)')|Out-Null
if($LASTEXITCODE -ne 0){throw 'Runner ancestor traversal grant failed.'}
Set-SiteAcl $base @($runnerRx,$daemonRx)
foreach($relative in @('private','monitor-private')){Set-SiteAcl (Join-Path $base $relative)}
foreach($relative in @('daemon-private','relay-private')){Set-SiteAcl (Join-Path $base $relative) @($daemonRx)}
Set-SiteAcl (Join-Path $base 'verification') @('*S-1-5-19:(OI)(CI)M')
foreach($relative in @('runtime/logs','runtime/scratch','probe/business/results','canary/business/results')){Set-SiteAcl (Join-Path $base $relative) @($runnerModify,'*S-1-5-19:(OI)(CI)M')}
foreach($relative in @('probe/business/workers','canary/business/workers')){Set-SiteAcl (Join-Path $base $relative)}
$probe=@{read_files=@($registry,(Join-Path $base 'config/broker.key'),$publisher,(Join-Path $base 'probe/business/task/00/1.png'),(Join-Path $base 'probe/business/refs/arknights/icons/marker.png'));denied_read_files=@('C:\ProgramData\coord\data\coord.db',(Join-Path $base 'verification/state.db'),(Join-Path $base 'daemon-private/management.key'),(Join-Path $base 'monitor-private/management.key'),(Join-Path $base 'relay-private/identity.key'))+@(0..5|ForEach-Object{Join-Path $base ('probe/business/workers/'+$_.ToString('0000')+'/attempts/private.json')});denied_write_files=@($registry,$publisher,(Join-Path $base 'bin/coord.exe'));write_directories=@($scratch,(Join-Path $base 'runtime/logs'),(Join-Path $base 'probe/business/results'));denied_write_directories=@((Join-Path $base 'verification'),(Join-Path $base 'config'),(Join-Path $base 'bin'),$env:SystemRoot);denied_rpc_url='http://127.0.0.1:17777/';broker_key_file=(Join-Path $base 'config/broker.key')}
$probe|ConvertTo-Json -Depth 10|Set-Content -LiteralPath (Join-Path $base 'config/acl-probe.json') -Encoding utf8NoBOM
Servy -Arguments @('install','--name',$probeService,'--path',(Join-Path $base 'bin/coord.exe'),'--startupDir',$base,'--params',('serve --addr 127.0.0.1:17777 --db "'+(Join-Path $base 'verification/state.db')+'" --actions-config "'+$registry+'"'),'--user','NT AUTHORITY\LocalService','--startupType','Manual','--stdout',(Join-Path $base 'runtime/logs/probe-daemon.stdout.log'),'--stderr',(Join-Path $base 'runtime/logs/probe-daemon.stderr.log'),'--quiet') -Label 'probe-service-install'
$previousPassword=[Environment]::GetEnvironmentVariable('SERVY_PASSWORD','Process')
try{
 [Environment]::SetEnvironmentVariable('SERVY_PASSWORD',$credential.GetNetworkCredential().Password,'Process')
 Servy -Arguments @('install','--name',$runnerService,'--path',(Join-Path $base 'bin/coord-action-runner.exe'),'--startupDir',$base,('--params=--registry "'+$registry+'" --check-identity'),'--user',($env:COMPUTERNAME+'\'+$userName),'--startupType','Manual','--stdout',(Join-Path $base 'runtime/logs/runner.stdout.log'),'--stderr',(Join-Path $base 'runtime/logs/runner.stderr.log'),'--quiet') -Label 'runner-service-install'
}finally{[Environment]::SetEnvironmentVariable('SERVY_PASSWORD',$previousPassword,'Process')}
& sc.exe privs $runnerService SeChangeNotifyPrivilege *> (Join-Path $base 'private/runner-required-privileges.log')
if($LASTEXITCODE -ne 0){throw 'Runner SCM minimum-privilege configuration failed.'}
$state=@{state='PREPARED';commit=$Commit;base=$base;backup_root=$BackupRoot;runner_sid=$runnerSid;daemon_sid=$daemonSid;runner_service=$runnerService;probe_service=$probeService;registry=$registry;probe_url='http://127.0.0.1:17777/';credential_storage='SCM plus administrator-only DPAPI credential; transient installer environment cleared';artifact_files=$manifest.files;production_replacement='NOT_RUN'}
$state|ConvertTo-Json -Depth 10|Set-Content -LiteralPath (Join-Path $base 'private/site-state.json') -Encoding utf8NoBOM
$state|ConvertTo-Json -Depth 10
