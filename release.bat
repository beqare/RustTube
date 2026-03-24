@echo off
setlocal

cd /d "%~dp0"

call ".\build.bat"
if errorlevel 1 (
    echo.
    echo Release build stopped because build.bat failed.
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
    echo Then run release.bat again.
    exit /b 1
)

echo.
echo Running Inno Setup compiler...
"%ISCC%" ".\setup.iss"
if errorlevel 1 (
    echo.
    echo Installer build failed.
    exit /b 1
)

echo.
echo Release finished successfully.
echo Installer output should be in dist\installer
exit /b 0
