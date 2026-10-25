# Exact-site formal profile preparation. Never claims classification tasks.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$base='C:\ProgramData\coord\g7'
$pointer=Join-Path $base 'private/initial-operation.json'
$state=Get-Content -Raw $pointer|ConvertFrom-Json
if($state.state -ne 'INPUTS_DEPLOYED'){throw 'Fresh validated inputs required.'}
$backup=$state.backup_root
$registry=Join-Path $base 'config/registry-production.json'
$candidate=Join-Path $base 'config/registry-initial-candidate.json'
if(Test-Path $candidate){throw 'Existing candidate requires reconciliation.'}
$config=Get-Content -Raw $registry|ConvertFrom-Json
if($config.profiles.'emote-publisher-v1'){throw 'Profile already exists.'}
$profile=$config.profiles.'g7-browser-v1'|ConvertTo-Json -Depth 20|ConvertFrom-Json
foreach($kind in @('on_task_completed','on_batch_completed')){
 $command=$profile.$kind
 if($command.sha256 -ne 'cac5496ed1dea50f837f28403feb62515de1ba1459ea2557abe6097ec07bec55' -or (Get-FileHash $command.executable).Hash.ToLowerInvariant() -ne $command.sha256){throw 'Approved publisher hash mismatch.'}
 $command.args=@('--root',(Join-Path $base 'initial/business'))
}
$config.profiles|Add-Member -NotePropertyName 'emote-publisher-v1' -NotePropertyValue $profile
$config|ConvertTo-Json -Depth 30|Set-Content $candidate -Encoding utf8NoBOM
$servy='C:\Program Files\Servy\servy-cli.exe'
$original=Get-Content -Raw (Join-Path $backup 'CoordActionRunnerG7.json')|ConvertFrom-Json
$probeService=$original|ConvertTo-Json -Depth 30|ConvertFrom-Json
$probeService.Parameters='--registry "'+$candidate+'" --probe-acl "'+(Join-Path $base 'config/initial-acl-probe.json')+'"'
$probeService.EnableHealthMonitoring=$false
$probeConfig=Join-Path $backup 'runner-acl-probe-service.json'
$probeService|ConvertTo-Json -Depth 30|Set-Content $probeConfig -Encoding utf8NoBOM
$log=Join-Path $base 'runtime/logs/runner.stderr.log'
$probeLog=Join-Path $backup 'formal-acl-probe.stderr.log'
$result=@{state='FAIL';backup_root=$backup}
function Import-Runner([string]$Path){
 & $servy import --config json --path $Path --quiet *> (Join-Path $backup 'runner-import.log')
 if($LASTEXITCODE -ne 0){throw 'Owned Runner import failed.'}
}
try{
 Stop-Service CoordActionRunnerG7
 Copy-Item $log (Join-Path $backup 'runner-before-profile.stderr.log')
 [IO.File]::WriteAllText($log,'')
 Import-Runner $probeConfig
 & $servy start --name CoordActionRunnerG7 --quiet *> (Join-Path $backup 'probe-start.log')
 $text=''
 foreach($n in 1..120){
  if(Test-Path $log){$text=[IO.File]::ReadAllText($log)}
  if($text.Contains('"acl_probe":"PASS"')){break}
  Start-Sleep -Milliseconds 200
 }
 if((Get-Service CoordActionRunnerG7).Status -ne 'Stopped'){Stop-Service CoordActionRunnerG7}
 Copy-Item $log $probeLog
 if(-not $text.Contains('"acl_probe":"PASS"')){throw 'Actual low-privilege input/output ACL probe failed.'}
 $identity=$text.Split("`n")[0]|ConvertFrom-Json
 if($identity.sid -ne $config.external_runner.runner_sid -or $identity.privileged -ne $false){throw 'Runner identity failed.'}
 # Every deployed input inherits the same protected read-only root ACL.
 $manifest=Get-Content -Raw (Join-Path $base 'initial/business/deployment-manifest.json')|ConvertFrom-Json
 $bad=@()
 foreach($file in $manifest.files){
  $acl=Get-Acl -LiteralPath (Join-Path $base ('initial/business/'+$file.path))
  foreach($rule in $acl.Access){
   $sid=$rule.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value
   if($rule.AccessControlType -eq 'Allow' -and ($sid -notin @('S-1-5-18','S-1-5-32-544','S-1-5-19',$config.external_runner.runner_sid) -or ($sid -in @('S-1-5-19',$config.external_runner.runner_sid) -and ([int]$rule.FileSystemRights -band 278)))){$bad+=$file.path}
  }
 }
 if($bad.Count){throw 'Input file ACL permits unexpected access/write.'}
 $result.acl_files_checked=$manifest.files.Count
 $result.actual_runner_probe=$text
 Import-Runner (Join-Path $backup 'CoordActionRunnerG7.json')
 Disable-ScheduledTask coord-health-monitor|Out-Null
 try{
  [IO.File]::Move($candidate,$registry,$true)
  Restart-Service coord
  Start-Service CoordActionRunnerG7
 }finally{Enable-ScheduledTask coord-health-monitor|Out-Null}
 $key=[IO.File]::ReadAllText((Join-Path $base 'daemon-private/management.key')).Trim()
 $body='{"jsonrpc":"2.0","id":1,"method":"batches/status","params":{"batch_id":"d2313c78-22da-4cb8-bcd7-ece9897e0db1"}}'
 $ready=$null
 foreach($n in 1..60){try{$ready=Invoke-RestMethod 'http://127.0.0.1:7777/' -Method Post -ContentType application/json -Headers @{Authorization=('Bearer '+$key)} -Body $body;break}catch{Start-Sleep -Milliseconds 200}}
 if($ready.result.total -ne 4 -or $ready.result.active -ne 0){throw 'Authenticated daemon readiness failed.'}
 Start-ScheduledTask coord-health-monitor
 $state.state='PROFILE_INSTALLED'
 $state|ConvertTo-Json -Depth 20|Set-Content $pointer -Encoding utf8NoBOM
 $result.state='PASS';$result.profile=$profile;$result.profile_id='emote-publisher-v1'
 $result.registry_sha256=(Get-FileHash $registry).Hash.ToLowerInvariant()
 $result.root_acl=(Get-Acl (Join-Path $base 'initial')).Sddl
 $result.results_acl=(Get-Acl (Join-Path $base 'initial/business/results')).Sddl
}catch{
 if((Get-Service CoordActionRunnerG7).Status -ne 'Stopped'){Stop-Service CoordActionRunnerG7}
 Copy-Item (Join-Path $backup 'registry-before.json') $registry -Force
 Import-Runner (Join-Path $backup 'CoordActionRunnerG7.json')
 Restart-Service coord;Start-Service CoordActionRunnerG7
 Enable-ScheduledTask coord-health-monitor|Out-Null
 $result.failure=$_.Exception.Message;$result.rollback='Prior registry/Runner config restored; live DB preserved.'
}finally{
 $result|ConvertTo-Json -Depth 25|Set-Content (Join-Path $backup 'profile-result.json') -Encoding utf8NoBOM
 $result|ConvertTo-Json -Depth 25
}
if($result.state -ne 'PASS'){throw 'Formal profile gate failed; stop operation.'}
