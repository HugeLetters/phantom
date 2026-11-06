# Rune for the UI, pinned to a dev revision

The GUI is built with Rune, a pre-release, GPU-rendered Rust UI framework, pinned to a specific `dev` revision and updated deliberately. It was picked for its freedom of drawing (arbitrary shapes, effects, springs), which the Persona-inspired themes need, and for headless rendering, so the UI can be tested and screenshotted without a window. Every view is Rune code, so moving away from Rune would mean rewriting the UI; we accept that risk.
