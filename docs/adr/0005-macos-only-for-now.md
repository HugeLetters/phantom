# macOS only, for now

Phantom targets macOS only: the app is installed as an `.app` bundle by `scripts/install-mac.sh`, opens magnet links and files through macOS Apple events, and reveals files in Finder. The service, protocol and CLI are portable Rust and would run elsewhere; other platforms would mainly need their own installer and platform integration in the GUI.
