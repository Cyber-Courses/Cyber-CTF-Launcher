# Sends a release to the Microsoft Store through the Partner Center submission API: the package
# (an x64 + Arm64 .msixbundle) and the whole listing, every language in packaging/store/listings/ with its
# texts, screenshots, logos and trailer. Partner Center then shows a new submission:
#   -Commit $true   submits it for certification (release pipeline);
#   -Commit $false  leaves it as a draft to review and submit by hand in Partner Center.
#
# Env: STORE_TENANT_ID, STORE_CLIENT_ID, STORE_CLIENT_SECRET, STORE_PRODUCT_ID.
# API reference: https://learn.microsoft.com/windows/uwp/monetize/manage-app-submissions
param(
  [Parameter(Mandatory)] [string] $Package,     # path to the .msixbundle
  [Parameter(Mandatory)] [string] $StoreDir,    # the repo's packaging/store/ folder
  [bool] $Commit = $false
)
$ErrorActionPreference = "Stop"
$api = "https://manage.devcenter.microsoft.com/v1.0/my/applications/$env:STORE_PRODUCT_ID"

function Get-Headers {
  $token = (Invoke-RestMethod -Method Post -Uri "https://login.microsoftonline.com/$env:STORE_TENANT_ID/oauth2/token" -Body @{
      grant_type = "client_credentials"; client_id = $env:STORE_CLIENT_ID; client_secret = $env:STORE_CLIENT_SECRET
      resource = "https://manage.devcenter.microsoft.com"
    }).access_token
  @{ Authorization = "Bearer $token" }
}
function Invoke-Store([string] $Method, [string] $Uri, $Body = $null) {
  $params = @{ Method = $Method; Uri = $Uri; Headers = (Get-Headers); ContentType = "application/json; charset=utf-8" }
  if ($null -ne $Body) { $params.Body = [Text.Encoding]::UTF8.GetBytes(($Body | ConvertTo-Json -Depth 30)) }
  $raw = Invoke-WebRequest @params
  if ($raw.Content) { $raw.Content | ConvertFrom-Json -AsHashtable } else { $null }
}

# Everything to upload goes flat into one zip; the submission refers to files by name.
$stage = Join-Path $env:RUNNER_TEMP "store-upload"
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $stage | Out-Null
function Add-File([string] $Source, [string] $Name) {
  Copy-Item $Source (Join-Path $stage $Name)
  return $Name
}

Write-Host "Creating a submission (a copy of the published one)"
$sub = Invoke-Store Post "$api/submissions"
$id = $sub.id
Write-Host "Submission $id"

# Package: the new bundle replaces every package of the copied submission.
foreach ($p in $sub.applicationPackages) { $p.fileStatus = "PendingDelete" }
$sub.applicationPackages += @{ fileName = (Add-File $Package "CyberCTF.msixbundle"); fileStatus = "PendingUpload" }

# Image types this script owns; any other image (set by hand in Partner Center) is left as is.
$managed = @("Screenshot", "StoreLogo9x16", "StoreLogoSquare", "Icon")
$common = Get-Content (Join-Path $StoreDir "listings/common.json") -Raw | ConvertFrom-Json -AsHashtable
# What the certification team reads first (why the restricted capabilities are needed).
if ($common.notesForCertification) { $sub.notesForCertification = $common.notesForCertification }
$trailers = @()

foreach ($file in Get-ChildItem (Join-Path $StoreDir "listings") -Filter "*-*.json") {
  $lang = $file.BaseName                       # en-us, fr-fr
  $l = Get-Content $file.FullName -Raw | ConvertFrom-Json -AsHashtable
  # A language without its own images yet uses the English ones.
  $img = Join-Path $StoreDir "images/$lang"
  if (-not (Test-Path $img)) { $img = Join-Path $StoreDir "images/en-us" }
  Write-Host "Listing $lang"

  if (-not $sub.listings) { $sub.listings = @{} }
  if (-not $sub.listings.ContainsKey($lang)) { $sub.listings[$lang] = @{ baseListing = @{ images = @() }; platformOverrides = @{} } }
  $b = $sub.listings[$lang].baseListing
  foreach ($k in "title", "shortDescription", "description", "releaseNotes", "features", "keywords", "copyrightAndTrademarkInfo", "websiteUrl", "privacyPolicy", "supportContact") {
    $b[$k] = $l[$k]
  }

  $images = @()
  foreach ($i in @($b.images)) {
    if ($null -eq $i) { continue }
    if ($managed -contains $i.imageType) { $i.fileStatus = "PendingDelete" }
    $images += $i
  }
  foreach ($s in $l.screenshots) {
    $name = Add-File (Join-Path $img $s.file) "$lang-$($s.file)"
    $images += @{ fileName = $name; fileStatus = "PendingUpload"; imageType = "Screenshot"; description = $s.caption }
  }
  $images += @{ fileName = (Add-File (Join-Path $img $l.poster) "$lang-poster.png"); fileStatus = "PendingUpload"; imageType = "StoreLogo9x16" }
  $images += @{ fileName = (Add-File (Join-Path $StoreDir "images/common/$($common.boxArt)") "$lang-box-art.png"); fileStatus = "PendingUpload"; imageType = "StoreLogoSquare" }
  $images += @{ fileName = (Add-File (Join-Path $StoreDir "images/common/$($common.tile300)") "$lang-tile-300.png"); fileStatus = "PendingUpload"; imageType = "Icon" }
  $b.images = $images

  # One trailer per language, shown on that language's page (its title and thumbnail live there).
  $t = Join-Path $StoreDir "trailers/$lang"
  if (-not (Test-Path $t)) { $t = Join-Path $StoreDir "trailers/en-us" }
  if ($l.trailer -and (Test-Path (Join-Path $t $l.trailer.video))) {
    $trailers += @{
      videoFileName = (Add-File (Join-Path $t $l.trailer.video) "$lang-trailer.mp4")
      trailerAssets = @{ $lang = @{ title = $l.trailer.title; imageList = @(@{ fileName = (Add-File (Join-Path $t $l.trailer.thumbnail) "$lang-trailer-thumbnail.png"); description = $l.trailer.title }) } }
    }
  }
}
# The trailers are exactly the ones in the repo (a trailer left out of the list is deleted).
$sub.trailers = $trailers

Write-Host "Updating submission $id"
Invoke-Store Put "$api/submissions/$id" $sub | Out-Null

Write-Host "Uploading $((Get-ChildItem $stage).Count) files"
$zip = Join-Path $env:RUNNER_TEMP "store-upload.zip"
Remove-Item $zip -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip
# The upload URL is an Azure blob SAS; '+' in it must be escaped.
Invoke-WebRequest -Method Put -Uri ($sub.fileUploadUrl -replace '\+', '%2B') -Headers @{ "x-ms-blob-type" = "BlockBlob" } -InFile $zip | Out-Null

if (-not $Commit) {
  Write-Host "::notice::Submission $id is ready as a draft: review it in Partner Center (Cyber CTF > the open submission) and submit it there."
  exit 0
}

Write-Host "Submitting $id for certification"
Invoke-Store Post "$api/submissions/$id/commit" | Out-Null
do {
  Start-Sleep -Seconds 20
  $status = Invoke-Store Get "$api/submissions/$id/status"
  Write-Host "Status: $($status.status)"
} while ($status.status -eq "CommitStarted")
if ($status.status -eq "CommitFailed") {
  Write-Host "::error::The Store refused submission $($id): $(($status.statusDetails | ConvertTo-Json -Depth 10))"
  exit 1
}
Write-Host "Submission $id is in certification ($($status.status))."
