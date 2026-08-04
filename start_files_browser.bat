@echo off
chcp 65001 >nul
cd /d "%~dp0"

:: start_files_browser.bat - Launch files_browser
set PORT=8899
set ROOT=D:/
set BASE_PATH=/docs

echo [files_browser] Starting on port %PORT% serving %ROOT%

:: prefer release, fallback debug
set EXE=target\release\files_browser.exe
if not exist "%EXE%" set EXE=target\debug\files_browser.exe

if not exist "%EXE%" (
    echo [!] Building release...
    cargo build --release 2>&1
    set EXE=target\release\files_browser.exe
)

if not exist "%EXE%" (
    echo [ERROR] Build failed - exe not found: %EXE%
    pause
    exit /b 1
)

:: Kill any existing instance
taskkill /f /im files_browser.exe >nul 2>&1

echo [OK] Starting: %EXE% %PORT% %ROOT% %BASE_PATH%
start "Files Browser" "%EXE%" %PORT% %ROOT% %BASE_PATH%

echo [OK] http://localhost:%PORT%%BASE_PATH%/  (via Caddy: http://localhost:8000%BASE_PATH%)
exit /b 0
