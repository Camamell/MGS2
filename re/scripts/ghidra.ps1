# Runs a Ghidra post-script against the project's SLUS_201.44 program.
# Usage: re\scripts\ghidra.ps1 <Script.java> <args...>
param([Parameter(Mandatory)][string]$Script, [Parameter(ValueFromRemainingArguments)][string[]]$Rest)
$env:JAVA_HOME = "D:\Tools\jdk21\jdk21.0.12_12"
$env:PATH = "$env:JAVA_HOME\bin;$env:PATH"
$re = Split-Path $PSScriptRoot -Parent
& "D:\Tools\ghidra-official\ghidra_12.1.4_PUBLIC\support\analyzeHeadless.bat" "$re\ghidra" mgs2 `
    -process SLUS_201.44 -noanalysis -scriptPath "$re\scripts" -postScript $Script @Rest 2>&1 |
    Select-String -Pattern "ERROR|Exception|SCRIPT" | ForEach-Object { $_.Line }
