# Kwel Connect

Kwel Connect is the official integration layer for searching Kwel from local applications. It provides a Native Host and JSON CLI for macOS, Windows, and Ubuntu, plus the official **Kwel for Alfred** workflow.

Requirements: Google Chrome and a version of [Kwel](https://kwel.ryu.zone) that supports Local Connection. Alfred integration also requires macOS and Alfred 5 Powerpack.

## Installation

Download the installer for your operating system from [GitHub Releases](https://github.com/Masa-Ryu/kwel-connect/releases).

| OS | Installer |
| --- | --- |
| macOS 13 or later (Apple Silicon / Intel) | `KwelConnect-macos-universal.pkg` |
| Windows x64 | `KwelConnect-windows-x64-setup.exe` |
| Ubuntu 24.04 or later, amd64 | `KwelConnect-ubuntu-amd64.deb` |
| Ubuntu 24.04 or later, arm64 | `KwelConnect-ubuntu-arm64.deb` |

On macOS, open the pkg to install into your home folder. On Windows, open the exe to install for the current user. On Ubuntu, open the appropriate deb with your package installer. Native Host registration is automatic; you do not need to install Rust.

The macOS requirement follows [Chrome's current system requirements](https://support.google.com/chrome/answer/95411).

Before updating, turn Local Connection OFF in the Chrome profile you use. Run the installer, then turn Local Connection back ON after the update.

Enable **Local Connection** in Kwel Settings. If Chrome displays a permission prompt, review it before approving.

The downloads are unsigned, and macOS downloads are not notarized. Gatekeeper or SmartScreen may block them. Verify the release source before deciding whether to proceed.

### If macOS displays a warning on first launch

Kwel Connect for macOS is unsigned and not notarized. macOS may block individual executables the first time they run.

The affected executables are:

- `kwel-native-host`: Runs when Chrome connects through Native Messaging.
- `kwel-bridge`: Used when running the local CLI.
- `kwel-alfred`: Runs when searching through the Alfred Workflow.

These files start at different times, so each executable may require a separate confirmation on first use.

Before proceeding, verify that the files came from the official [GitHub Releases](https://github.com/Masa-Ryu/kwel-connect/releases) and match the published SHA-256 checksums.

For an unidentified-developer or notarization warning, after trying to launch the affected executable, you can decide whether to allow it through **System Settings → Privacy & Security → Open Anyway**, as described by Apple. Make this decision separately for each executable.

For other security warnings, such as malware being detected or software that will damage your computer, do not proceed with approval. Check the exact warning text.

Apple's official instructions: [Safely open apps on your Mac](https://support.apple.com/en-us/102445).

## Kwel for Alfred

1. Double-click **Kwel.alfredworkflow** from Releases to import it into Alfred.
2. Copy the Profile ID from Kwel Settings and paste it into the Workflow's **Configure Workflow… → Kwel Profile ID** field.
3. Add **Search Kwel** in Alfred Preferences → **Features → Default Results → Setup fallback searches**.

Type a query such as `Kyoto` in Alfred and choose **Search Kwel**. Select a result and press Return to open the page in that Chrome profile. No prefix or additional hotkey is required.

If you only use Alfred, the first search automatically registers the Host, so you do not need to install the pkg beforehand. Removing the Workflow leaves the shared Host installed.

## Uninstallation

Turn Local Connection OFF, then uninstall using **Kwel Connect Uninstaller** in `~/Library/Application Support/Kwel/NativeBridge/` on macOS, the app list in Windows Settings, or your package manager on Ubuntu. The uninstaller refuses to remove a Host that is in use. Your Kwel history and profiles are preserved.

## For launcher developers

[Local Client Protocol v1](docs/local-client-protocol-v1.md) can be used from other languages. See also the [Native Messaging specification](docs/native-messaging-v1.md), [security documentation](docs/security.md), and [development guide](docs/development.md).

Official support covers the Host for the three operating systems and the Alfred Workflow. Third-party launcher developers are responsible for the implementation, quality, and behavior of their launchers.
