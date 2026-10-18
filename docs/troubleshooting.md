# Troubleshooting

## "Boxdeck can't find Vagrant"

Apps started from Finder, the Dock or the Start menu don't always get your shell's `PATH`. Boxdeck imports your login shell's `PATH` at startup; if Vagrant still isn't found, set its full path in **Settings → Vagrant program**, then click **Look again**.

## A provider isn't found, or "No usable default provider"

Provider tools (`vmrun`, `VBoxManage`, …) are looked up on the same `PATH`. Add their folder under **Settings → Extra PATH folders**. For VMware, Vagrant also needs the `vagrant-vmware-desktop` plugin and the **Vagrant VMware Utility** service; see [install.md](install.md#vmware-fusion-on-macos-workstation-on-windowslinux).

## `DriverAPICertificateError` with VMware

The Vagrant VMware Utility isn't installed (it's separate from the plugin). On macOS: `brew install --cask vagrant-vmware-utility`; elsewhere, use HashiCorp's installer.

## "Port collision" although nothing uses the port

If `vagrant up` reports a forwarded-port collision for every port it tries (`the ports in the auto-correction range are all also used`), something on your computer is answering every local connection. VPN, proxy and security software can do this. Vagrant then believes every port is taken. Pause that software, or remove `forwarded_port` lines and reach the VM by its own IP address. This is outside Boxdeck; plain `vagrant up` in a terminal fails the same way.

## A job fails asking for a password (for example NFS on macOS)

Boxdeck runs Vagrant without a terminal, so prompts fail instead of hanging. Open the job's log and click **Run in terminal** to repeat it in a terminal tab where you can type the password.

## Hyper-V on Windows

Hyper-V needs Vagrant to run as administrator. Start Boxdeck with "Run as administrator".

## No tray icon on Linux

GNOME needs the AppIndicator extension to show tray icons. Everything in the tray is also available in the main window.

## "Boxdeck can't be opened" on macOS, or a SmartScreen warning on Windows

Early releases aren't code-signed yet. On macOS, right-click the app, choose **Open**, then **Open** again. On Windows, choose **More info → Run anyway**.

## Agents can't connect

Check **Settings → MCP / Agents**: agent access must be on, the status line should say "Listening on …", and the client must send its own token. If the port is taken, Boxdeck says so instead of picking another one; choose a free port and update your client.
