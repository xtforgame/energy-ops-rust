param(
  [Parameter(Mandatory=$true)][ValidatePattern('^[a-zA-Z0-9_-]+$')][string]$Project,
  [Parameter(Mandatory=$true)][string]$Destination
)
$ErrorActionPreference = 'Stop'
function Run-Docker([string[]]$Arguments) {
  $result = & docker @Arguments
  if ($LASTEXITCODE -ne 0) { throw "Docker failed: $($Arguments[0])" }
  return $result
}
$target = [IO.Path]::GetFullPath($Destination)
if (Test-Path -LiteralPath $target) { throw 'Destination must be a new directory; backups are never overwritten.' }
New-Item -ItemType Directory -Path $target | Out-Null
$ids = @(Run-Docker @('ps','-aq','--filter',"label=com.docker.compose.project=$Project"))
if (!$ids.Count) { throw 'Compose project not found.' }
$containers = @(Run-Docker (@('inspect') + $ids) | ConvertFrom-Json)
$db = @($containers | Where-Object { $_.Config.Labels.'com.docker.compose.service' -eq 'postgres' })
$objectStore = @($containers | Where-Object { $_.Config.Labels.'com.docker.compose.service' -eq 'minio' })
if ($db.Count -ne 1 -or $objectStore.Count -ne 1) { throw 'Expected exactly one postgres and minio service.' }
$mount = @($objectStore[0].Mounts | Where-Object { $_.Destination -eq '/data' -and $_.Type -eq 'volume' })
if ($mount.Count -ne 1) { throw 'MinIO /data named volume not found.' }
$dbEnv = @{}
foreach ($line in $db[0].Config.Env) { $pair = $line.Split('=',2); $dbEnv[$pair[0]] = $pair[1] }
if (!$dbEnv.POSTGRES_USER -or !$dbEnv.POSTGRES_DB) { throw 'Database user/name unavailable.' }
$writers = @($containers | Where-Object { $_.State.Running -and $_.Config.Labels.'com.docker.compose.service' -in @('platform','ocr') })
$stopped = @()
try {
  foreach ($c in $writers) { Run-Docker @('stop','--time','60',$c.Id) | Out-Null; $stopped += $c.Id }
  Run-Docker @('exec',$db[0].Id,'pg_dump','-U',$dbEnv.POSTGRES_USER,'-d',$dbEnv.POSTGRES_DB,'-Fc','-f','/tmp/energy-backup.dump') | Out-Null
  Run-Docker @('cp',"$($db[0].Id):/tmp/energy-backup.dump",(Join-Path $target 'database.dump')) | Out-Null
  # Only this project's immutable objects are archived; unrelated services stay running.
  Run-Docker @('run','--rm','--pull','never','--network','none','--mount',"type=volume,source=$($mount[0].Name),target=/objects,readonly",'--mount',"type=bind,source=$target,target=/backup",$db[0].Config.Image,'tar','-czf','/backup/objects.tar.gz','-C','/objects','.') | Out-Null
  $files = @('database.dump','objects.tar.gz') | ForEach-Object { $p=Join-Path $target $_; @{file=$_;sha256=(Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash;bytes=(Get-Item -LiteralPath $p).Length} }
  @{createdAt=(Get-Date).ToUniversalTime().ToString('o');project=$Project;databaseUser=$dbEnv.POSTGRES_USER;databaseName=$dbEnv.POSTGRES_DB;files=$files;services=@($containers | ForEach-Object {@{service=$_.Config.Labels.'com.docker.compose.service';image=$_.Config.Image;imageId=$_.Image}});configurationIncluded=$false;note='Keep compose/configuration/secrets separately under restricted access. Restore to an isolated empty environment before production recovery.'} | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $target 'manifest.json') -Encoding utf8
} finally {
  foreach ($id in $stopped) { Run-Docker @('start',$id) | Out-Null }
}
Write-Output "Backup completed: $target"
