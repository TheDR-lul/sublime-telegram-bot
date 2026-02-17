# Installation Instructions

## PostgreSQL Installation Status

PostgreSQL installation is in progress via winget. After it completes:

1. **Disable auto-start for PostgreSQL service:**
   ```powershell
   # Run as Administrator
   .\setup-services.ps1
   ```

   Or manually:
   ```powershell
   # Find PostgreSQL service
   $service = Get-Service -Name "*postgresql*" | Select-Object -First 1
   
   # Stop if running
   Stop-Service -Name $service.Name -Force
   
   # Set to Manual (no auto-start)
   Set-Service -Name $service.Name -StartupType Manual
   ```

2. **Start PostgreSQL service when needed:**
   ```powershell
   Start-Service -Name $service.Name
   ```

## Docker Desktop Installation

Docker Desktop installation via winget failed. Please install manually:

1. **Download Docker Desktop:**
   - Go to https://www.docker.com/products/docker-desktop/
   - Download Docker Desktop for Windows

2. **Install Docker Desktop:**
   - Run the installer
   - **Important:** During installation, uncheck "Start Docker Desktop when you log in"
   - Or after installation: Settings > General > Uncheck "Start Docker Desktop when you log in"

3. **Start Docker when needed:**
   - Launch Docker Desktop from Start menu
   - Wait for it to start (whale icon in system tray)

## After Both Are Installed

1. **Start PostgreSQL service:**
   ```powershell
   Start-Service -Name (Get-Service -Name "*postgresql*" | Select-Object -First 1).Name
   ```

2. **Create database:**
   ```powershell
   # Default installation path
   & "C:\Program Files\PostgreSQL\17\bin\psql.exe" -U postgres -c "CREATE DATABASE sublime_bot;"
   ```

3. **Update config.toml with your PostgreSQL password:**
   ```toml
   database_url = "postgresql://postgres:YOUR_PASSWORD@localhost:5432/sublime_bot"
   ```

4. **Apply migrations:**
   ```powershell
   cargo run -- migrate
   ```

5. **Run the bot:**
   ```powershell
   cargo run
   ```
