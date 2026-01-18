name := 'cosmic-bing-wallpaper'
appid := 'io.github.reality2_roycdavies.cosmic-bing-wallpaper'
destdir := ''

# Default recipe: build release
default: build-release

# Build in debug mode
build-debug:
    cargo build

# Build in release mode
build-release:
    cargo build --release

# Run in debug mode
run:
    cargo run

# Run in release mode
run-release:
    cargo run --release

# Check code with clippy
check:
    cargo clippy --all-features

# Format code
fmt:
    cargo fmt

# Clean build artifacts
clean:
    cargo clean

# Install to system (requires destdir variable, e.g., just destdir=/usr install)
install:
    install -Dm0755 target/release/{{name}} {{destdir}}/usr/bin/{{name}}
    install -Dm0644 resources/{{appid}}.desktop {{destdir}}/usr/share/applications/{{appid}}.desktop
    install -Dm0644 resources/{{appid}}.svg {{destdir}}/usr/share/icons/hicolor/scalable/apps/{{appid}}.svg
    install -Dm0644 resources/{{appid}}-symbolic.svg {{destdir}}/usr/share/icons/hicolor/symbolic/apps/{{appid}}-symbolic.svg
    install -Dm0644 resources/org.cosmicbing.Wallpaper1.service {{destdir}}/usr/share/dbus-1/services/org.cosmicbing.Wallpaper1.service

# Install to local user (with icon and desktop entry)
install-local:
    #!/bin/bash
    set -e

    # Stop running instances before upgrading
    echo "Stopping any running instances..."
    systemctl --user stop cosmic-bing-wallpaper-daemon.service 2>/dev/null || true
    systemctl --user stop cosmic-bing-wallpaper-tray.service 2>/dev/null || true
    pkill -f "cosmic-bing-wallpaper" 2>/dev/null || true
    sleep 1

    # Install binary (remove old first)
    mkdir -p ~/.local/bin
    rm -f ~/.local/bin/{{name}}
    cp target/release/{{name}} ~/.local/bin/

    # Install desktop entry and icons
    mkdir -p ~/.local/share/applications
    cp resources/{{appid}}.desktop ~/.local/share/applications/
    mkdir -p ~/.local/share/icons/hicolor/scalable/apps
    cp resources/{{appid}}.svg ~/.local/share/icons/hicolor/scalable/apps/
    mkdir -p ~/.local/share/icons/hicolor/symbolic/apps
    cp resources/{{appid}}-symbolic.svg ~/.local/share/icons/hicolor/symbolic/apps/
    cp resources/{{appid}}-on-symbolic.svg ~/.local/share/icons/hicolor/symbolic/apps/
    cp resources/{{appid}}-off-symbolic.svg ~/.local/share/icons/hicolor/symbolic/apps/

    # Create D-Bus service file with expanded home path
    mkdir -p ~/.local/share/dbus-1/services
    cat > ~/.local/share/dbus-1/services/org.cosmicbing.Wallpaper1.service << EOF
    [D-BUS Service]
    Name=org.cosmicbing.Wallpaper1
    Exec=$HOME/.local/bin/cosmic-bing-wallpaper --daemon
    EOF

    # Restart services if they were enabled
    if systemctl --user is-enabled cosmic-bing-wallpaper-daemon.service 2>/dev/null; then
        echo "Restarting daemon service..."
        systemctl --user start cosmic-bing-wallpaper-daemon.service
    fi
    if systemctl --user is-enabled cosmic-bing-wallpaper-tray.service 2>/dev/null; then
        echo "Restarting tray service..."
        systemctl --user start cosmic-bing-wallpaper-tray.service
    fi

    echo "Installation complete!"

# Uninstall from local user
uninstall-local:
    rm -f ~/.local/bin/{{name}}
    rm -f ~/.local/share/applications/{{appid}}.desktop
    rm -f ~/.local/share/icons/hicolor/scalable/apps/{{appid}}.svg
    rm -f ~/.local/share/icons/hicolor/symbolic/apps/{{appid}}-symbolic.svg
    rm -f ~/.local/share/icons/hicolor/symbolic/apps/{{appid}}-on-symbolic.svg
    rm -f ~/.local/share/icons/hicolor/symbolic/apps/{{appid}}-off-symbolic.svg
    rm -f ~/.local/share/dbus-1/services/org.cosmicbing.Wallpaper1.service

# Install with system tray autostart (uses systemd for COSMIC desktop)
install-with-tray: install-local
    #!/bin/bash
    mkdir -p ~/.config/systemd/user

    # Create D-Bus daemon service (runs on login, provides IPC for tray/GUI sync)
    cat > ~/.config/systemd/user/cosmic-bing-wallpaper-daemon.service << 'EOF'
    [Unit]
    Description=Bing Wallpaper D-Bus daemon for COSMIC desktop
    After=dbus.socket
    Requires=dbus.socket

    [Service]
    Type=simple
    ExecStart=%h/.local/bin/cosmic-bing-wallpaper --daemon
    Restart=on-failure
    RestartSec=5

    [Install]
    WantedBy=default.target
    EOF

    # Create tray service (runs on login, after daemon) - uses %h specifier for portability
    cat > ~/.config/systemd/user/cosmic-bing-wallpaper-tray.service << 'EOF'
    [Unit]
    Description=Bing Wallpaper system tray for COSMIC desktop
    After=cosmic-session.target cosmic-bing-wallpaper-daemon.service
    Wants=cosmic-bing-wallpaper-daemon.service
    PartOf=cosmic-session.target

    [Service]
    Type=simple
    ExecStart=%h/.local/bin/cosmic-bing-wallpaper --tray
    Restart=on-failure
    RestartSec=5

    [Install]
    WantedBy=cosmic-session.target
    EOF

    # Create daily fetch service - uses %h and %U specifiers for portability
    cat > ~/.config/systemd/user/cosmic-bing-wallpaper.service << 'EOF'
    [Unit]
    Description=Fetch and set Bing daily wallpaper for COSMIC desktop
    After=network-online.target graphical-session.target
    Wants=network-online.target

    [Service]
    Type=oneshot
    ExecStart=%h/.local/bin/cosmic-bing-wallpaper --fetch-and-apply
    Environment=HOME=%h
    Environment=XDG_RUNTIME_DIR=/run/user/%U

    [Install]
    WantedBy=default.target
    EOF

    # Create daily timer
    cat > ~/.config/systemd/user/cosmic-bing-wallpaper.timer << 'EOF'
    [Unit]
    Description=Daily Bing wallpaper update timer

    [Timer]
    OnCalendar=*-*-* 08:00:00
    OnBootSec=5min
    RandomizedDelaySec=300
    Persistent=true

    [Install]
    WantedBy=timers.target
    EOF

    # Create login/wake service (runs on graphical session start)
    cat > ~/.config/systemd/user/cosmic-bing-wallpaper-login.service << 'EOF'
    [Unit]
    Description=Fetch Bing wallpaper on login/wake
    After=graphical-session.target network-online.target
    Wants=network-online.target

    [Service]
    Type=oneshot
    ExecStartPre=/bin/sleep 10
    ExecStart=%h/.local/bin/cosmic-bing-wallpaper --fetch-and-apply
    Environment=HOME=%h
    Environment=XDG_RUNTIME_DIR=/run/user/%U

    [Install]
    WantedBy=graphical-session.target
    EOF

    systemctl --user daemon-reload
    systemctl --user enable cosmic-bing-wallpaper-daemon.service
    systemctl --user enable cosmic-bing-wallpaper-tray.service
    systemctl --user enable --now cosmic-bing-wallpaper.timer
    systemctl --user enable cosmic-bing-wallpaper-login.service

    # Start services now
    echo "Starting services..."
    systemctl --user start cosmic-bing-wallpaper-daemon.service
    systemctl --user start cosmic-bing-wallpaper-tray.service

    echo "Daemon, tray, daily update timer, and login trigger installed, enabled, and started."

# Uninstall including tray autostart
uninstall-with-tray: uninstall-local
    systemctl --user stop cosmic-bing-wallpaper-daemon.service 2>/dev/null || true
    systemctl --user stop cosmic-bing-wallpaper-tray.service 2>/dev/null || true
    systemctl --user stop cosmic-bing-wallpaper.timer 2>/dev/null || true
    systemctl --user disable cosmic-bing-wallpaper-daemon.service 2>/dev/null || true
    systemctl --user disable cosmic-bing-wallpaper-tray.service 2>/dev/null || true
    systemctl --user disable cosmic-bing-wallpaper.timer 2>/dev/null || true
    systemctl --user disable cosmic-bing-wallpaper-login.service 2>/dev/null || true
    rm -f ~/.config/systemd/user/cosmic-bing-wallpaper-daemon.service
    rm -f ~/.config/systemd/user/cosmic-bing-wallpaper-tray.service
    rm -f ~/.config/systemd/user/cosmic-bing-wallpaper.service
    rm -f ~/.config/systemd/user/cosmic-bing-wallpaper.timer
    rm -f ~/.config/systemd/user/cosmic-bing-wallpaper-login.service
    systemctl --user daemon-reload

# Build and run
br: build-debug run
