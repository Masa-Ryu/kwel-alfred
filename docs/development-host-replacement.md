# Replacing an unreleased development Host

This is a one-time manual procedure for developers who installed the old local
Go build. Ordinary users of the first Rust release follow the [README](../README.md).
No replacement logic is present in the runtime or installer.

1. In every test Chrome profile, turn off Local Connection (called Alfred
   Integration in old test builds). Quit Chrome normally. In Activity Monitor,
   verify the old Host has exited. If it remains, identify its executable path
   and owner before stopping only that process. Do not terminate another
   user's Host or another launcher's shared Rust Host.
2. In Finder, inspect the files in
   `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/`.
   Development builds may have registered `com.kwel.alfred.json`, or manually
   used `com.kwel.bridge.json`. Open each candidate as text and inspect its
   executable path; the old build used
   `~/Library/Application Support/Kwel/AlfredBridge/bin/kwel-bridge`.
   Check ownership with Get Info, confirm the files and path components are
   not symlinks, and match them to the build you installed. A canonical
   registration pointing to `Kwel/NativeBridge` belongs to the Rust Host.
3. Only after verifying an owned old registration and stopped executable,
   move those old registration files to a private backup directory outside
   NativeMessagingHosts. Back up the verified AlfredBridge directory as well.
   Do not delete an unknown registration, a symlink, a file owned by another
   user, or a path belonging to a different product. Stop and inspect manually
   if ownership or identity is uncertain. Do not touch Kwel's browsing data or
   Profile ID settings. Stale `/private/tmp/kwa-<uid>` development files are
   not used by Rust and need not be removed.
4. Install the Rust Host or import the new Alfred Workflow following the
   README. Enable Local Connection again and explicitly approve any browser
   prompt. Confirm `com.kwel.bridge.json` points to the installed Rust Host.
   Keep the Profile ID supplied by Kwel, search for `Kyoto`, then open a result.

An old Go Host left running can continue serving its old clients until stopped;
the new Rust client neither discovers it nor coordinates its ownership. Stop it
before using the Rust installation. Installers refuse an existing canonical
manifest with an unrecognized executable instead of silently replacing it.
The old alias is never rewritten or removed automatically.

Windows and Ubuntu have no released Go distribution in this repository. If a
manually constructed development registration exists, disable the extension,
stop only the identified owned Host, inspect its registry value/manifest and
executable, and back it up before unregistering it. Do not use broad registry,
directory or process deletion. Then use the normal platform installer.
