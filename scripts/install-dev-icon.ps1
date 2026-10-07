# Windows: no separate icon install for `cargo run`.
# build.rs embeds assets/logo/Isengard.ico into the .exe (Explorer / taskbar / Alt+Tab).
Write-Host "Windows: icon is embedded by build.rs on cargo build/run. Nothing to install."
Write-Host "Prerequisites: rustup (MSVC toolchain) + VS Build Tools C++ workload — see docs/architecture.md"
Write-Host "Then: cargo run"
