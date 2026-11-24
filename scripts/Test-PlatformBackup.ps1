param([Parameter(Mandatory=$true)][string]$BackupDirectory,[string]$PostgresImage='postgres:18')
$ErrorActionPreference='Stop'
function Run-Docker([string[]]$Arguments) { $r=& docker @Arguments; if($LASTEXITCODE -ne 0){throw "Docker failed: $($Arguments[0])"}; return $r }
$source=[IO.Path]::GetFullPath($BackupDirectory)
$manifest=Get-Content -LiteralPath (Join-Path $source 'manifest.json') -Raw | ConvertFrom-Json
foreach($f in $manifest.files){
  if($f.file -notin @('database.dump','objects.tar.gz')){throw 'Unexpected backup file.'}
  if((Get-FileHash -LiteralPath (Join-Path $source $f.file) -Algorithm SHA256).Hash -ne $f.sha256){throw "Backup checksum mismatch: $($f.file)"}
}
if(@($manifest.files).Count -ne 2){throw 'Incomplete backup manifest.'}
$tag='energy-restore-test-'+[guid]::NewGuid().ToString('N').Substring(0,12)
$dbVolume=$tag+'-db';$objectsVolume=$tag+'-objects';$made=@();$started=$false
try {
  foreach($v in @($dbVolume,$objectsVolume)){Run-Docker @('volume','create',$v)|Out-Null;$made+=$v}
  $password=[guid]::NewGuid().ToString('N')
  Run-Docker @('run','-d','--pull','never','--name',$tag,'--network','none','-e',"POSTGRES_PASSWORD=$password",'-e','POSTGRES_USER=restore_test','-e','POSTGRES_DB=restore_test','--mount',"type=volume,source=$dbVolume,target=/var/lib/postgresql",$PostgresImage)|Out-Null;$started=$true
  $ready=$false
  for($i=0;$i -lt 30;$i++){ & docker exec $tag pg_isready -U restore_test -d restore_test *> $null; if($LASTEXITCODE -eq 0){$ready=$true;break};Start-Sleep -Seconds 1 }
  if(!$ready){throw 'Temporary database did not start.'}
  Run-Docker @('cp',(Join-Path $source 'database.dump'),"${tag}:/tmp/database.dump")|Out-Null
  Run-Docker @('exec',$tag,'pg_restore','--exit-on-error','--no-owner','--no-privileges','-U','restore_test','-d','restore_test','/tmp/database.dump')|Out-Null
  Run-Docker @('run','--rm','--pull','never','--network','none','--mount',"type=bind,source=$source,target=/backup,readonly",'--mount',"type=volume,source=$objectsVolume,target=/objects",$PostgresImage,'tar','-xzf','/backup/objects.tar.gz','-C','/objects')|Out-Null
  $counts=Run-Docker @('exec',$tag,'psql','-U','restore_test','-d','restore_test','-At','-c',"SELECT json_build_object('stations',(SELECT count(*) FROM tbl_energy_station),'bills',(SELECT count(*) FROM tbl_energy_bill),'attachments',(SELECT count(*) FROM tbl_energy_attachment));")
  $objects=Run-Docker @('run','--rm','--pull','never','--network','none','--mount',"type=volume,source=$objectsVolume,target=/objects,readonly",$PostgresImage,'sh','-c','find /objects -type f | wc -l')
  $result=@{at=(Get-Date).ToUniversalTime().ToString('o');databaseRestored=$true;objectsRestored=$true;counts=($counts|ConvertFrom-Json);objectFileCount=[int]$objects;applicationStarted=$false;note='Isolated database and object-volume restore; customer boot and full application recovery still require target-environment validation.'}
  $result|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $source 'restore-test.json') -Encoding utf8
  $result|ConvertTo-Json -Depth 5
} finally {
  if($started){& docker rm -f $tag|Out-Null}
  foreach($v in $made){& docker volume rm $v|Out-Null}
}
