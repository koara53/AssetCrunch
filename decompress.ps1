param([string]$InputPath)
$OutputPath = $InputPath + "_restored"
$log = "C:\Users\ze00r\Desktop\decompress_debug.txt"
"InputPath: [$InputPath]" | Out-File $log -Encoding UTF8
"OutputPath: [$OutputPath]" | Out-File $log -Append -Encoding UTF8
"OutputPath exists before: $(Test-Path $OutputPath)" | Out-File $log -Append -Encoding UTF8

$exe = "C:\Users\ze00r\Desktop\gpu-compress\assetcrunch.exe"
Start-Process -FilePath $exe -ArgumentList "decompress-folder", "`"$InputPath`"", "`"$OutputPath`"" -Wait -NoNewWindow

"OutputPath exists after: $(Test-Path $OutputPath)" | Out-File $log -Append -Encoding UTF8
if (Test-Path $OutputPath) {
    Get-ChildItem $OutputPath | Select-Object Name | Out-File $log -Append -Encoding UTF8
}