# Shared SSH target for deploy/update scripts (dot-source from repo root).
# Set environment variable SUBLIME_SSH_TARGET to "user@host" to point all scripts at your server
# Default prod: root@5.189.154.71. Override for another host, e.g.: $env:SUBLIME_SSH_TARGET = "root@1.2.3.4"

function Get-SublimeSshTarget {
    $fromEnv = $env:SUBLIME_SSH_TARGET
    if ($null -ne $fromEnv -and $fromEnv.Trim().Length -gt 0) {
        return $fromEnv.Trim()
    }
    return "root@5.189.154.71"
}
