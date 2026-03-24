@echo off
setlocal

cd /d "%~dp0\.."

set "MODE=%~1"

if "%MODE%"=="" (
    echo.
    echo Select an action:
    echo   1. Build app package
    echo   2. Build app package ^+ installer
    echo.
    set /p MODE=Enter 1 or 2: 
)

if "%MODE%"=="1" goto :build_only
if "%MODE%"=="2" goto :build_release

echo.
echo Invalid selection. Please run again and choose 1 or 2.
exit /b 1

:build_only
powershell -ExecutionPolicy Bypass -File ".\scripts\package-release.ps1"
if errorlevel 1 (
    echo.
    echo Build failed.
    exit /b 1
)

echo.
echo Build finished successfully.
exit /b 0

:build_release
call "%~f0" 1
if errorlevel 1 (
    echo.
    echo Release build stopped because the app build failed.
    exit /b 1
)

set "ISCC="

if exist "%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe" set "ISCC=%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe"
if not defined ISCC if exist "%ProgramFiles%\Inno Setup 6\ISCC.exe" set "ISCC=%ProgramFiles%\Inno Setup 6\ISCC.exe"

if not defined ISCC (
    for %%I in (ISCC.exe) do set "ISCC=%%~$PATH:I"
)

if not defined ISCC (
    echo.
    echo Inno Setup Compiler not found.
    echo Install Inno Setup 6 or add ISCC.exe to PATH.
    echo Then run build.bat again.
    exit /b 1
)

echo.
echo Running Inno Setup compiler...
"%ISCC%" ".\scripts\setup.iss"
if errorlevel 1 (
    echo.
    echo Installer build failed.
    exit /b 1
)

echo.
echo Release finished successfully.
echo Installer output should be in dist\installer
exit /b 0
