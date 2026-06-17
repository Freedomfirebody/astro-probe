# Astro-Probe E2E Test Execution Script
# Run this script from the project root or the visualizers directory to bootstrap, build, and run tests.

Write-Host "=== Starting Astro-Probe Visualizers Bootstrap, Build & Verification ===" -ForegroundColor Cyan

$VisualizersDir = Join-Path $PSScriptRoot ""
if (-not (Test-Path (Join-Path $VisualizersDir "package.json"))) {
    $VisualizersDir = Join-Path $PSScriptRoot "visualizers"
}

Write-Host "1. Installing root coordinates..." -ForegroundColor Yellow
cd $VisualizersDir
npm install

Write-Host "2. Running bootstrapping for Server and Frontend..." -ForegroundColor Yellow
npm run bootstrap

Write-Host "3. Installing E2E test suite dependencies..." -ForegroundColor Yellow
cd (Join-Path $VisualizersDir "e2e-tests")
npm install

Write-Host "4. Compiling React Frontend (Production Build)..." -ForegroundColor Yellow
cd (Join-Path $VisualizersDir "frontend")
npm run build

Write-Host "5. Running End-to-End Test Suite..." -ForegroundColor Yellow
cd (Join-Path $VisualizersDir "e2e-tests")
node test-runner.js

Write-Host "=== Astro-Probe Verification Completed ===" -ForegroundColor Green
