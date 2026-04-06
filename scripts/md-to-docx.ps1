[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$InputPath,
    [Parameter(Mandatory = $true)]
    [string]$OutputPath
)

$ErrorActionPreference = "Stop"

function Escape-Xml([string]$Value) {
    if ($null -eq $Value) {
        return ""
    }
    return [System.Security.SecurityElement]::Escape($Value)
}

function Clean-Inline([string]$Value) {
    if ($null -eq $Value) {
        return ""
    }

    $Value = $Value -replace '\*\*(.*?)\*\*', '$1'
    $Value = $Value -replace '`([^`]+)`', '$1'
    $Value = [System.Text.RegularExpressions.Regex]::Replace(
        $Value,
        '\[([^\]]+)\]\(([^\)]+)\)',
        '$1'
    )
    return $Value
}

$inputFullPath = (Resolve-Path $InputPath).Path
$outputFullPath = [System.IO.Path]::GetFullPath($OutputPath)
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$tempRoot = Join-Path $repoRoot ".tmp_spec_review_docx"

if (Test-Path $tempRoot) {
    Remove-Item -Recurse -Force $tempRoot
}

New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $tempRoot "_rels") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $tempRoot "word") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $tempRoot "word\_rels") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $tempRoot "docProps") | Out-Null

$text = [System.IO.File]::ReadAllText($inputFullPath, [System.Text.UTF8Encoding]::new($false))
$lines = $text -split "`r?`n"

$body = New-Object System.Text.StringBuilder
[void]$body.AppendLine('<w:body>')

foreach ($rawLine in $lines) {
    $line = $rawLine.TrimEnd()
    if ([string]::IsNullOrWhiteSpace($line)) {
        [void]$body.AppendLine('<w:p/>')
        continue
    }

    $style = $null
    $textLine = $line

    if ($line.StartsWith('### ')) {
        $style = 'Heading3'
        $textLine = $line.Substring(4)
    } elseif ($line.StartsWith('## ')) {
        $style = 'Heading2'
        $textLine = $line.Substring(3)
    } elseif ($line.StartsWith('# ')) {
        $style = 'Heading1'
        $textLine = $line.Substring(2)
    }

    $textLine = Clean-Inline $textLine
    $textLine = Escape-Xml $textLine

    if ($style) {
        [void]$body.AppendLine("<w:p><w:pPr><w:pStyle w:val=""$style""/></w:pPr><w:r><w:t xml:space=""preserve"">$textLine</w:t></w:r></w:p>")
    } else {
        [void]$body.AppendLine("<w:p><w:r><w:t xml:space=""preserve"">$textLine</w:t></w:r></w:p>")
    }
}

[void]$body.AppendLine('<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr>')
[void]$body.AppendLine('</w:body>')

$documentXml = @"
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:wpc="http://schemas.microsoft.com/office/word/2010/wordprocessingCanvas" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:v="urn:schemas-microsoft-com:vml" xmlns:wp14="http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:w10="urn:schemas-microsoft-com:office:word" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" xmlns:wpg="http://schemas.microsoft.com/office/word/2010/wordprocessingGroup" xmlns:wpi="http://schemas.microsoft.com/office/word/2010/wordprocessingInk" xmlns:wne="http://schemas.microsoft.com/office/word/2006/wordml" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape" mc:Ignorable="w14 wp14">
$body
</w:document>
"@

$stylesXml = @'
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:style w:type="paragraph" w:default="1" w:styleId="Normal">
    <w:name w:val="Normal"/>
    <w:qFormat/>
    <w:rPr><w:sz w:val="22"/></w:rPr>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Heading1">
    <w:name w:val="heading 1"/>
    <w:basedOn w:val="Normal"/>
    <w:next w:val="Normal"/>
    <w:qFormat/>
    <w:rPr><w:b/><w:sz w:val="32"/></w:rPr>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Heading2">
    <w:name w:val="heading 2"/>
    <w:basedOn w:val="Normal"/>
    <w:next w:val="Normal"/>
    <w:qFormat/>
    <w:rPr><w:b/><w:sz w:val="28"/></w:rPr>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Heading3">
    <w:name w:val="heading 3"/>
    <w:basedOn w:val="Normal"/>
    <w:next w:val="Normal"/>
    <w:qFormat/>
    <w:rPr><w:b/><w:sz w:val="24"/></w:rPr>
  </w:style>
</w:styles>
'@

$contentTypes = @'
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
  <Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
  <Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>
</Types>
'@

$rootRels = @'
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>
  <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/>
</Relationships>
'@

$docRels = @'
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
</Relationships>
'@

$coreXml = @'
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:dcmitype="http://purl.org/dc/dcmitype/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <dc:title>OmniDrive Spec Review</dc:title>
  <dc:creator>Codex</dc:creator>
  <cp:lastModifiedBy>Codex</cp:lastModifiedBy>
  <dcterms:created xsi:type="dcterms:W3CDTF">2026-03-28T00:00:00Z</dcterms:created>
  <dcterms:modified xsi:type="dcterms:W3CDTF">2026-03-28T00:00:00Z</dcterms:modified>
</cp:coreProperties>
'@

$appXml = @'
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes">
  <Application>Codex</Application>
</Properties>
'@

[System.IO.File]::WriteAllText((Join-Path $tempRoot '[Content_Types].xml'), $contentTypes, [System.Text.UTF8Encoding]::new($false))
[System.IO.File]::WriteAllText((Join-Path $tempRoot '_rels\.rels'), $rootRels, [System.Text.UTF8Encoding]::new($false))
[System.IO.File]::WriteAllText((Join-Path $tempRoot 'word\document.xml'), $documentXml, [System.Text.UTF8Encoding]::new($false))
[System.IO.File]::WriteAllText((Join-Path $tempRoot 'word\styles.xml'), $stylesXml, [System.Text.UTF8Encoding]::new($false))
[System.IO.File]::WriteAllText((Join-Path $tempRoot 'word\_rels\document.xml.rels'), $docRels, [System.Text.UTF8Encoding]::new($false))
[System.IO.File]::WriteAllText((Join-Path $tempRoot 'docProps\core.xml'), $coreXml, [System.Text.UTF8Encoding]::new($false))
[System.IO.File]::WriteAllText((Join-Path $tempRoot 'docProps\app.xml'), $appXml, [System.Text.UTF8Encoding]::new($false))

if (Test-Path $outputFullPath) {
    Remove-Item -Force $outputFullPath
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
[System.IO.Compression.ZipFile]::CreateFromDirectory($tempRoot, $outputFullPath)

Get-Item $outputFullPath | Select-Object FullName,Length,LastWriteTime
