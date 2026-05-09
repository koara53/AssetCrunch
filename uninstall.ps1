# AssetCrunch 右クリックメニュー削除スクリプト
# 管理者権限で実行してください

$regBase = "Registry::HKEY_CLASSES_ROOT\Directory\shell\AssetCrunch"
$regBg   = "Registry::HKEY_CLASSES_ROOT\Directory\Background\shell\AssetCrunch"

if (Test-Path $regBase) {
    Remove-Item -Path $regBase -Recurse -Force
    Write-Host "フォルダメニューを削除しました。"
}

if (Test-Path $regBg) {
    Remove-Item -Path $regBg -Recurse -Force
    Write-Host "背景メニューを削除しました。"
}

Write-Host "アンインストール完了！" -ForegroundColor Green