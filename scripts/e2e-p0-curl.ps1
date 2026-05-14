# feedbackmonk P0 end-to-end curl pipeline (PowerShell variant).
#
# Mirrors `scripts/e2e-p0-curl.sh` for Windows-native PowerShell. Same
# preconditions, same exit semantics:
#   exit 0 -- full pipeline PASS
#   exit 1 -- a step failed
#   exit 2 -- a pre-condition is missing
#
# Drafted by CLAUDE-B in Stage 2; the .sh variant is the canonical witness
# (Stage 3 runs it as the P0-exit-gate witness). This .ps1 is for Windows
# developer ergonomics -- same logic, idiomatic PowerShell.
#
# Usage:
#   pwsh -File scripts/e2e-p0-curl.ps1
#
# Requires: curl.exe, openssl.exe, jq.exe on PATH. (Git for Windows ships
# curl + openssl; Scoop/Chocolatey provide jq.)

$ErrorActionPreference = 'Stop'

$ApiBase     = if ($env:FEEDBACKMONK_API_BASE) { $env:FEEDBACKMONK_API_BASE } else { 'http://127.0.0.1:14304' }
$MailpitBase = if ($env:MAILPIT_BASE)       { $env:MAILPIT_BASE }       else { 'http://127.0.0.1:8025' }
$TestEmail   = "e2e-$([DateTimeOffset]::Now.ToUnixTimeSeconds())@example.com"
$TestPassword= 'correct horse battery staple 9!'
$WorkDir     = Join-Path $env:TEMP "feedbackmonk-e2e-$([guid]::NewGuid())"
New-Item -ItemType Directory -Path $WorkDir | Out-Null
$KeysDir     = Join-Path $WorkDir 'keys'
New-Item -ItemType Directory -Path $KeysDir | Out-Null

function Log([string]$msg) { Write-Host "[$([datetime]::Now.ToString('HH:mm:ss'))] $msg" }
function Pass([string]$msg) { Log "PASS: $msg" }
function Fail([string]$msg) { Log "FAIL: $msg"; exit 1 }
function Need([string]$cmd) {
    if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) {
        Log "missing dep: $cmd"
        exit 2
    }
}

Need 'curl.exe'
Need 'openssl.exe'
Need 'jq.exe'

try { Invoke-WebRequest -UseBasicParsing "$ApiBase/health" -TimeoutSec 5 | Out-Null }
catch { Log "API not reachable at $ApiBase/health"; exit 2 }

try { Invoke-WebRequest -UseBasicParsing "$MailpitBase/api/v1/messages?limit=1" -TimeoutSec 5 | Out-Null }
catch { Log "Mailpit not reachable at $MailpitBase"; exit 2 }

Log "API: $ApiBase | Mailpit: $MailpitBase | work: $WorkDir"

# ---------- step 1: signup --------------------------------------------------

Log 'step 1: POST /api/v1/signup'
$signupBody = @{ email = $TestEmail; password = $TestPassword } | ConvertTo-Json -Compress
$signupResp = Invoke-RestMethod -Method Post -Uri "$ApiBase/api/v1/signup" -ContentType 'application/json' -Body $signupBody
$signupResp | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $WorkDir 'signup.json')
if (-not $signupResp.tenant_id) { Fail 'signup did not return tenant_id' }
Pass 'step 1 -- tenant created'

# ---------- step 2: read verify token from Mailpit + verify ----------------

Log 'step 2: read verify-email token from Mailpit + POST /api/v1/verify-email'
Start-Sleep -Seconds 1
$messages = Invoke-RestMethod -Uri "$MailpitBase/api/v1/messages?limit=50"
$ourMsg = $messages.messages | Where-Object { $_.To[0].Address -eq $TestEmail } | Select-Object -First 1
if (-not $ourMsg) { Fail "no Mailpit message for $TestEmail" }
$full = Invoke-RestMethod -Uri "$MailpitBase/api/v1/message/$($ourMsg.ID)"
$token = ([regex]'token=([A-Za-z0-9_-]+)').Match($full.Text).Groups[1].Value
if (-not $token) { Fail 'could not extract verify token from Mailpit body' }
Log "  token: $($token.Substring(0, [Math]::Min(8, $token.Length)))..."

$session = New-Object Microsoft.PowerShell.Commands.WebRequestSession
$verifyBody = @{ token = $token } | ConvertTo-Json -Compress
$verifyResp = Invoke-WebRequest -Method Post -Uri "$ApiBase/api/v1/verify-email" `
    -ContentType 'application/json' -Body $verifyBody -WebSession $session
if ($verifyResp.StatusCode -ne 200) { Fail "verify-email did not return 200: $($verifyResp.StatusCode)" }
$haveSessionCookie = $session.Cookies.GetCookies($ApiBase) | Where-Object { $_.Name -eq 'feedbackmonk_session' }
if (-not $haveSessionCookie) { Fail 'verify-email did not set feedbackmonk_session cookie' }
Pass 'step 2 -- email verified, session cookie set'

# ---------- step 3: create project -----------------------------------------

Log 'step 3: POST /api/v1/projects'
$projBody = @{ name = 'E2E Test Project'; slug = 'e2e-test' } | ConvertTo-Json -Compress
$projResp = Invoke-RestMethod -Method Post -Uri "$ApiBase/api/v1/projects" `
    -ContentType 'application/json' -Body $projBody -WebSession $session
$projResp | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $WorkDir 'project.json')
$ProjectId = $projResp.project_id
if (-not $ProjectId) { $ProjectId = $projResp.id }
if (-not $ProjectId) { Fail 'project create did not return project_id' }
Log "  project_id: $ProjectId"
Pass 'step 3 -- project created'

# ---------- step 4: register signing key -----------------------------------

Log "step 4: generate Ed25519 keypair + POST /api/v1/projects/$ProjectId/signing-keys"
$privPem = Join-Path $KeysDir 'ed25519_private.pem'
$pubBin  = Join-Path $KeysDir 'ed25519_public.bin'
& openssl.exe genpkey -algorithm ED25519 -out $privPem 2>$null
# DER pubkey is 12-byte prefix + 32 bytes; strip prefix.
$pubDer = & openssl.exe pkey -in $privPem -pubout -outform DER 2>$null | ForEach-Object { $_ }
$derBytes = (& openssl.exe pkey -in $privPem -pubout -outform DER 2>$null)
# Use file-based extraction since pipeline byte handling is brittle in PS.
$derPath = Join-Path $KeysDir 'pub.der'
& openssl.exe pkey -in $privPem -pubout -outform DER -out $derPath 2>$null
$der = [System.IO.File]::ReadAllBytes($derPath)
[System.IO.File]::WriteAllBytes($pubBin, $der[12..43])
$pubBase64 = [Convert]::ToBase64String([System.IO.File]::ReadAllBytes($pubBin))

$keyBody = @{ public_key_base64 = $pubBase64; label = 'e2e-key' } | ConvertTo-Json -Compress
$keyResp = Invoke-RestMethod -Method Post -Uri "$ApiBase/api/v1/projects/$ProjectId/signing-keys" `
    -ContentType 'application/json' -Body $keyBody -WebSession $session
$keyResp | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $WorkDir 'signing_key.json')
if (-not $keyResp.signing_key_id) { Fail 'signing-key register did not return signing_key_id' }
Pass 'step 4 -- signing key registered'

# ---------- step 5: mint JWT + auth-mode submission ------------------------

Log "step 5: mint EdDSA JWT and POST /api/v1/projects/$ProjectId/feedback (auth mode)"
$now = [DateTimeOffset]::Now.ToUnixTimeSeconds()
$exp = $now + 300

function Base64UrlNoPad([byte[]]$bytes) {
    [Convert]::ToBase64String($bytes).TrimEnd('=').Replace('+','-').Replace('/','_')
}
$headerJson = '{"alg":"EdDSA","typ":"JWT"}'
$payloadJson = @{
    sub = 'e2e-user-1'
    aud = $ProjectId
    iat = $now
    exp = $exp
    email = 'e2e-user-1@example.com'
    name  = 'E2E User'
} | ConvertTo-Json -Compress
$headerB64  = Base64UrlNoPad ([System.Text.Encoding]::UTF8.GetBytes($headerJson))
$payloadB64 = Base64UrlNoPad ([System.Text.Encoding]::UTF8.GetBytes($payloadJson))
$signingInput = "$headerB64.$payloadB64"
$siPath = Join-Path $WorkDir 'signing-input.bin'
[System.IO.File]::WriteAllBytes($siPath, [System.Text.Encoding]::UTF8.GetBytes($signingInput))
$sigPath = Join-Path $WorkDir 'sig.bin'
& openssl.exe pkeyutl -sign -inkey $privPem -rawin -in $siPath -out $sigPath
$sigB64 = Base64UrlNoPad ([System.IO.File]::ReadAllBytes($sigPath))
$jwt = "$signingInput.$sigB64"

$submitBody = @{ body = 'e2e auth-mode body'; kind = 'bug' } | ConvertTo-Json -Compress
$submitResp = Invoke-RestMethod -Method Post -Uri "$ApiBase/api/v1/projects/$ProjectId/feedback" `
    -ContentType 'application/json' -Headers @{ Authorization = "Bearer $jwt" } -Body $submitBody
$submitResp | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $WorkDir 'submit_auth.json')
if (-not $submitResp.feedback_id.StartsWith('FB-')) { Fail 'auth-mode submit did not return FB-XXXXXX' }
Pass 'step 5 -- auth-mode submission accepted'

# ---------- step 6: anonymous-mode submission ------------------------------

Log "step 6: POST /api/v1/projects/$ProjectId/feedback (anon mode)"
$anonBody = @{ body = 'e2e anon body'; kind = 'feature' } | ConvertTo-Json -Compress
$anonResp = Invoke-RestMethod -Method Post -Uri "$ApiBase/api/v1/projects/$ProjectId/feedback" `
    -ContentType 'application/json' -Body $anonBody
$anonResp | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $WorkDir 'submit_anon.json')
if (-not $anonResp.feedback_id.StartsWith('FB-')) { Fail 'anon-mode submit did not return FB-XXXXXX' }
Pass 'step 6 -- anon-mode submission accepted'

# ---------- step 7: rate-limit boundary ------------------------------------

Log 'step 7: 11 rapid anon submissions; 11th must 429'
$cookie = 'e2e-cookie-deterministic'
foreach ($i in 1..10) {
    $code = (Invoke-WebRequest -Method Post -Uri "$ApiBase/api/v1/projects/$ProjectId/feedback" `
        -ContentType 'application/json' `
        -Headers @{ 'X-Feedbackmonk-Anon-Cookie' = $cookie } `
        -Body (@{ body = "burst $i"; kind = 'other' } | ConvertTo-Json -Compress) `
        -SkipHttpErrorCheck).StatusCode
    if ($code -ne 200) { Fail "anon submission $i returned $code (expected 200)" }
}
$code11 = (Invoke-WebRequest -Method Post -Uri "$ApiBase/api/v1/projects/$ProjectId/feedback" `
    -ContentType 'application/json' `
    -Headers @{ 'X-Feedbackmonk-Anon-Cookie' = $cookie } `
    -Body (@{ body = 'burst 11'; kind = 'other' } | ConvertTo-Json -Compress) `
    -SkipHttpErrorCheck).StatusCode
if ($code11 -ne 429) { Fail "11th anon submission returned $code11 (expected 429)" }
Pass 'step 7 -- 11th submission correctly rate-limited'

Log "ALL STEPS PASSED. Witness artefacts: $WorkDir"
