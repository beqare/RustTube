@echo off
setlocal

cd /d "%~dp0\.."
powershell -ExecutionPolicy Bypass -File ".\scripts\package-release.ps1"

if errorlevel 1 (
    echo.
    echo Build failed.
    exit /b 1
)

echo.
echo Build finished successfully.
exit /b 0
