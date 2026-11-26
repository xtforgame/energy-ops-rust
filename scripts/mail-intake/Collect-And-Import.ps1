param(
    [Parameter(Mandatory=$true)][string]$CollectorDirectory,
    [string]$BaseUrl='http://127.0.0.1:11950',
    [int]$OrgId=1,
    [string]$StateDirectory='C:\EnergyOps\PlatformIntake',
    [switch]$Watch
)
$ErrorActionPreference='Stop'
$node=Join-Path $CollectorDirectory 'runtime\node.exe'
$collector=Join-Path $CollectorDirectory 'cli.mjs'
if(!(Test-Path -LiteralPath $node) -or !(Test-Path -LiteralPath $collector)){throw '找不到既有收信工具及內附 Node，請確認 CollectorDirectory。'}
$config=Get-Content -LiteralPath (Join-Path $CollectorDirectory 'config.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$ready=Join-Path $config.outputDirectory 'ready'
$credential=Get-Credential -Message '輸入平台帳號（不是 Outlook 信箱密碼）；僅本次執行使用，不寫入檔案'
if($null -eq $credential){throw '未輸入平台帳號，未開始。'}
$oldUser=$env:EO_INTAKE_USER
$oldPassword=$env:EO_INTAKE_PASSWORD
try {
    $env:EO_INTAKE_USER=$credential.UserName
    $env:EO_INTAKE_PASSWORD=$credential.GetNetworkCredential().Password
    do {
        & $node $collector run
        if($LASTEXITCODE -ne 0){Write-Warning '本輪收信未全部成功；仍匯入已下載完成的檔案，請查看收信工具紀錄。'}
        if(Test-Path -LiteralPath $ready){
            & $node (Join-Path $PSScriptRoot 'ingest-folder.mjs') --folder $ready --state $StateDirectory --base-url $BaseUrl --org-id $OrgId
            if($LASTEXITCODE -ne 0){Write-Warning '平台匯入有未完成項目，請查看 last-run.json。'}
        }
        if($Watch){Write-Host '本輪結束，30 分鐘後再檢查。Ctrl+C 停止；沒有自動核准或送出 EF。';Start-Sleep -Seconds 1800}
    } while($Watch)
} finally {
    $env:EO_INTAKE_USER=$oldUser
    $env:EO_INTAKE_PASSWORD=$oldPassword
    $credential=$null
}
