# AssetCrunch 右クリックメニュー登録スクリプト
# 管理者権限で実行してください

$exePath = "$PSScriptRoot\assetcrunch.exe"

if (-not (Test-Path $exePath)) {
    Write-Error "assetcrunch.exe が見つかりません: $exePath"
    exit 1
}

# フォルダの右クリックメニューに登録
$regBase = "Registry::HKEY_CLASSES_ROOT\Directory\shell\AssetCrunch"

New-Item -Path $regBase -Force | Out-Null
Set-ItemProperty -Path $regBase -Name "(Default)" -Value "AssetCrunch で圧縮"
Set-ItemProperty -Path $regBase -Name "Icon" -Value $exePath

New-Item -Path "$regBase\command" -Force | Out-Null
Set-ItemProperty -Path "$regBase\command" -Name "(Default)" `
    -Value "`"$exePath`" compress-folder `"%1`" `"%1_compressed`""

# 背景右クリック（フォルダ内の空白右クリック）にも登録
$regBg = "Registry::HKEY_CLASSES_ROOT\Directory\Background\shell\AssetCrunch"

New-Item -Path $regBg -Force | Out-Null
Set-ItemProperty -Path $regBg -Name "(Default)" -Value "AssetCrunch で圧縮"
Set-ItemProperty -Path $regBg -Name "Icon" -Value $exePath

New-Item -Path "$regBg\command" -Force | Out-Null
Set-ItemProperty -Path "$regBg\command" -Name "(Default)" `
    -Value "`"$exePath`" compress-folder `"%V`" `"%V_compressed`""

Write-Host "インストール完了！" -ForegroundColor Green
Write-Host "フォルダを右クリックすると「AssetCrunch で圧縮」が表示されます。"