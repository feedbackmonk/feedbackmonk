# Worker A smoke test -- exercise the binary end-to-end against Mailpit + Postgres dev container.
$ErrorActionPreference = 'Stop'
$env:DATABASE_URL = 'postgres://postgres:dev@localhost:5433/feedbackmonk_dev'
$env:FEEDBACKMONK_SESSION_SECRET = '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'
$env:FEEDBACKMONK_MAILER = 'mailpit'
$env:FEEDBACKMONK_PORT = '14304'
$env:FEEDBACKMONK_PUBLIC_URL = 'http://localhost:14304'

$proc = Start-Process -FilePath 'target\debug\feedbackmonk-api.exe' -PassThru -NoNewWindow -RedirectStandardOutput stdout.log -RedirectStandardError stderr.log
try {
    Start-Sleep -Seconds 3
    Write-Host "Server PID: $($proc.Id)"

    $health = Invoke-WebRequest -UseBasicParsing http://localhost:14304/health -TimeoutSec 5
    Write-Host "health: $($health.StatusCode) $($health.Content)"

    $signupBody = @{ email = "smoke@example.com"; password = "hunter22" } | ConvertTo-Json
    $signup = Invoke-WebRequest -UseBasicParsing -Method POST -Uri http://localhost:14304/api/v1/signup -ContentType 'application/json' -Body $signupBody
    Write-Host "signup: $($signup.StatusCode) $($signup.Content)"

    Start-Sleep -Seconds 1
    $msgs = Invoke-RestMethod -UseBasicParsing http://localhost:8025/api/v1/messages
    Write-Host "mailpit message count: $($msgs.total)"
    if ($msgs.total -gt 0) {
        $latest = $msgs.messages[0]
        Write-Host "  subject: $($latest.Subject)"
        Write-Host "  to:      $($latest.To.Address -join ',')"
    }
} finally {
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
}
