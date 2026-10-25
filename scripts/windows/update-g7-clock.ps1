param([Parameter(Mandatory)][string]$CandidateDirectory,[Parameter(Mandatory)][string]$ArtifactManifest)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$base='C:\ProgramData\coord\g7'
$private=Join-Path $base 'private'
$manifest=Get-Content -Raw $ArtifactManifest|ConvertFrom-Json
if($manifest.state -ne 'PASS' -or $manifest.commit -ne 'f0aa7daed53a36f8c0c2418d935858bbf9f9fd9e'){throw 'Matching corrected CI acceptance required.'}
foreach($entry in $manifest.files){if((Get-FileHash (Join-Path $CandidateDirectory $entry.name)).Hash.ToLowerInvariant() -ne $entry.sha256){throw 'Candidate hash mismatch.'}}
$identity=Join-Path $CandidateDirectory 'identity.cjs'
if((Get-FileHash $identity).Hash.ToLowerInvariant() -ne $manifest.identity_sha256){throw 'Corrected Relay source hash mismatch.'}
$binaryChanges=@($manifest.files|Where-Object{(Get-FileHash (Join-Path $base ('bin/'+$_.name))).Hash.ToLowerInvariant() -ne $_.sha256})
$summary=@{state='FAIL';commit=$manifest.commit;binary_changes=@($binaryChanges.name);manifest_hash=(Get-FileHash $ArtifactManifest).Hash.ToLowerInvariant()}
try{
 Stop-Service CoordRelayAgent
 if($binaryChanges.Count){
  $restore=Join-Path $private ('clock-before-'+[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ'))
  New-Item -ItemType Directory -Path $restore|Out-Null
  foreach($entry in $manifest.files){Copy-Item (Join-Path $base ('bin/'+$entry.name)) (Join-Path $restore $entry.name)}
  Copy-Item (Join-Path $base 'config/registry-production.json') (Join-Path $restore 'registry-production.json')
  $summary.restore_root=$restore
  $site=Get-Content -Raw (Join-Path $private 'site-state.json')|ConvertFrom-Json
  & 'C:\Users\lieyanbang\AppData\Local\Programs\Python\Python314\python.exe' (Join-Path $site.backup_root 'sqlite-online-backup.py') --source 'C:\ProgramData\coord\data\coord.db' --destination (Join-Path $restore 'accepted-state.db') *> (Join-Path $restore 'snapshot.json')
  if($LASTEXITCODE -ne 0){throw 'Accepted-state backup failed before corrected binary update.'}
  Disable-ScheduledTask coord-health-monitor|Out-Null;Stop-ScheduledTask coord-health-monitor
  Stop-Service CoordActionRunnerG7;Stop-Service coord
  foreach($entry in $manifest.files){Copy-Item (Join-Path $CandidateDirectory $entry.name) (Join-Path $base ('bin/'+$entry.name)) -Force}
  Copy-Item (Join-Path $CandidateDirectory 'coord.exe') 'C:\ProgramData\coord\bin\coord.exe' -Force
  $registryPath=Join-Path $base 'config/registry-production.json'
  $registry=Get-Content -Raw $registryPath|ConvertFrom-Json
  $publisherHash=($manifest.files|Where-Object name -eq 'coord-emote-action.exe').sha256
  foreach($profile in $registry.profiles.PSObject.Properties.Value){foreach($kind in @('on_task_completed','on_batch_completed')){if($profile.$kind){$profile.$kind.sha256=$publisherHash}}}
  $registry|ConvertTo-Json -Depth 30|Set-Content $registryPath -Encoding utf8NoBOM
  Start-Service coord;Start-Service CoordActionRunnerG7
  Enable-ScheduledTask coord-health-monitor|Out-Null;Start-ScheduledTask coord-health-monitor
 }
 Copy-Item $identity 'C:\ProgramData\coord-relay-agent\identity.cjs' -Force
 Start-Service CoordRelayAgent
 $summary.state='PASS';$summary.identity_sha256=(Get-FileHash 'C:\ProgramData\coord-relay-agent\identity.cjs').Hash.ToLowerInvariant();$summary.installed=@{}
 foreach($entry in $manifest.files){$summary.installed[$entry.name]=(Get-FileHash (Join-Path $base ('bin/'+$entry.name))).Hash.ToLowerInvariant()}
}finally{$summary|ConvertTo-Json -Depth 12|Set-Content (Join-Path $private 'clock-deployment.json') -Encoding utf8NoBOM;$summary|ConvertTo-Json -Depth 12}
if($summary.state -ne 'PASS'){throw 'Corrected deployment failed; use protected restore points.'}
