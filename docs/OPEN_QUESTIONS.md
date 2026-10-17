# Open questions

Each entry: question, why it matters, proposed experiment, status, answer.

1. Does `omarchy plugin validate` accept extra top-level manifest keys? (S5)
2. Can one plugin depend on or import another? (S6)
3. Can a service plugin create a Bottom-layer surface with free-form input regions without stealing focus or affecting lock behavior? (S1, S2)
4. Which theme singletons and components can third-party QML import, and are they stable? (S8)
5. How far does the manifest settings form go? (S9)
6. What do `omarchy.monitor` and `omarchy.agents` already expose? (S10)
7. Does `omarchy plugin add` accept a SHA or subdirectory? (S7)
8. Can the shell run headless in CI to render previews? (S11)
9. What does the live marketplace contain, and where are the gaps? (S12)
10. Is the name acceptable to the Omarchy maintainers? (human task)
11. Keyring behavior under Omarchy 4 autologin: what can another same-uid process read? (S4)
12. Persistent Unix socket from Quickshell, and reconnect across `rescanPlugins`? (S3)
