# Explicit recovery procedure; never run automatically after accepted work.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$base='C:\ProgramData\coord\g7'
$state=Get-Content -Raw (Join-Path $base 'private/site-state.json')|ConvertFrom-Json
$backup=$state.backup_root
$recovery=Join-Path $base ('private/rollback-'+[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ'))
New-Item -ItemType Directory -Path $recovery|Out-Null
Disable-ScheduledTask coord-health-monitor|Out-Null
Stop-ScheduledTask coord-health-monitor
foreach($name in @('CoordActionRunnerG7','CoordRelayAgent','coord')){
 $service=Get-Service $name
 if($service.Status -ne 'Stopped'){Stop-Service $name;$service.WaitForStatus('Stopped',[timespan]::FromSeconds(30))}
}
# Save current SQLite/WAL contents after quiescing before restoring any code.
& 'C:\Users\lieyanbang\AppData\Local\Programs\Python\Python314\python.exe' (Join-Path $backup 'sqlite-online-backup.py') --source 'C:\ProgramData\coord\data\coord.db' --destination (Join-Path $recovery 'accepted-state.db') *> (Join-Path $recovery 'snapshot.json')
if($LASTEXITCODE -ne 0){throw 'Accepted-state backup failed; do not continue rollback.'}
Copy-Item (Join-Path $backup 'binary/coord.exe') 'C:\ProgramData\coord\bin\coord.exe' -Force
& 'C:\Program Files\Servy\servy-cli.exe' import --config json --path (Join-Path $backup 'services/coord.json') --quiet *> (Join-Path $recovery 'coord-import.log')
if($LASTEXITCODE -ne 0){throw 'Prior coord configuration restore failed.'}
Get-ChildItem (Join-Path $backup 'relay') -File|Copy-Item -Destination 'C:\ProgramData\coord-relay-agent' -Force
Copy-Item (Join-Path $backup 'monitor/coord-health-monitor.ps1') 'C:\ProgramData\coord\health\coord-health-monitor.ps1' -Force
Register-ScheduledTask -TaskName coord-health-monitor -Xml (Get-Content -Raw (Join-Path $backup 'monitor/coord-health-monitor.xml')) -Force|Out-Null
Set-Service CoordActionRunnerG7 -StartupType Manual
Start-Service coord;Start-Service CoordRelayAgent
Start-ScheduledTask coord-health-monitor
@{state='CODE_RESTORED';accepted_database_preserved=$true;accepted_state_snapshot=(Join-Path $recovery 'accepted-state.db');runner='stopped/manual';external_results='preserved; inspect receipts before any replay';note='Old binary additive-schema compatibility was rehearsed. No stale database restore.'}|ConvertTo-Json
