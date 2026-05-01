# Copy your SSH public key to the server so deploy no longer asks for password.
# Run once, enter password when prompted.

. "$PSScriptRoot\scripts\DeploySshTarget.ps1"
$Target = Get-SublimeSshTarget
$KeyPath = "$env:USERPROFILE\.ssh\id_ed25519.pub"

if (-not (Test-Path $KeyPath)) {
    Write-Host "Generating new SSH key (no passphrase)..." -ForegroundColor Cyan
    ssh-keygen -t ed25519 -f "$env:USERPROFILE\.ssh\id_ed25519" -N '""'
}
$pubKey = Get-Content $KeyPath -Raw
Write-Host "Copying key to $Target (SUBLIME_SSH_TARGET overrides default server; enter password once)..." -ForegroundColor Cyan
$pubKey | ssh -o StrictHostKeyChecking=accept-new $Target "mkdir -p .ssh && chmod 700 .ssh && cat >> .ssh/authorized_keys && chmod 600 .ssh/authorized_keys"
if ($LASTEXITCODE -eq 0) {
    Write-Host "Done. Next time deploy will not ask for password." -ForegroundColor Green
} else {
    Write-Error "Failed. Check password and connection."
}
