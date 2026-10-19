# Installing

Boxdeck is a front end for HashiCorp Vagrant: it runs the `vagrant` you have installed. You need three things:

1. **Vagrant** 2.3 or newer.
2. **A provider**: the software that actually runs the VMs (VMware, VirtualBox, Parallels, libvirt, Hyper-V or Docker), plus any Vagrant plugin it needs.
3. **Boxdeck** itself.

## 1. Install Vagrant

| System                 | Command                                                                            |
| ---------------------- | ---------------------------------------------------------------------------------- |
| macOS (Homebrew)       | `brew tap hashicorp/tap && brew install hashicorp/tap/hashicorp-vagrant`           |
| Windows (winget)       | `winget install Hashicorp.Vagrant`                                                 |
| Ubuntu / Debian        | Add HashiCorp's apt repository, then `sudo apt install vagrant` (see below)        |
| Fedora / RHEL / others | Follow [HashiCorp's install page](https://developer.hashicorp.com/vagrant/install) |

On Ubuntu or Debian:

```bash
wget -O - https://apt.releases.hashicorp.com/gpg | sudo gpg --dearmor -o /usr/share/keyrings/hashicorp-archive-keyring.gpg
echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/hashicorp-archive-keyring.gpg] https://apt.releases.hashicorp.com $(lsb_release -cs) main" | sudo tee /etc/apt/sources.list.d/hashicorp.list
sudo apt update && sudo apt install vagrant
```

Check it: `vagrant --version` should print 2.3 or newer.

## 2. Set up a provider

Pick the one you use. Vagrant needs both the virtualization software and, for some providers, a plugin.

### VMware (Fusion on macOS, Workstation on Windows/Linux)

VMware needs **three** pieces:

1. VMware Fusion (macOS) or VMware Workstation Pro (Windows, Linux), installed and opened once.
2. The Vagrant plugin:
   ```bash
   vagrant plugin install vagrant-vmware-desktop
   ```
3. The **Vagrant VMware Utility**, a small system service the plugin talks to. It's separate from the plugin and easy to miss.
   - macOS: `brew install --cask vagrant-vmware-utility`
   - Windows and Linux: download it from [HashiCorp's VMware page](https://developer.hashicorp.com/vagrant/install/vmware) and run the installer.

   The installer needs administrator rights. Without the Utility, Vagrant reports `DriverAPICertificateError` and Boxdeck shows that error on the project.

Use it with `config.vm.provider "vmware_desktop"` in your Vagrantfile, or set `VAGRANT_DEFAULT_PROVIDER=vmware_desktop` in Boxdeck's Settings → Environment variables.

### VirtualBox

Install [VirtualBox](https://www.virtualbox.org/wiki/Downloads) (on macOS: `brew install --cask virtualbox`). No plugin is needed. On Apple Silicon Macs, use VirtualBox 7.1 or newer and arm64 boxes.

### Parallels (macOS)

Install Parallels Desktop (Pro or Business edition), then:

```bash
vagrant plugin install vagrant-parallels
```

### libvirt (Linux)

Install libvirt and QEMU from your distribution, then the plugin. See the [vagrant-libvirt install guide](https://vagrant-libvirt.github.io/vagrant-libvirt/installation.html) for the build dependencies, then:

```bash
vagrant plugin install vagrant-libvirt
```

### Hyper-V (Windows)

Turn on the Hyper-V Windows feature. Vagrant supports it without a plugin, but it must run as administrator, so start Boxdeck with "Run as administrator".

### Docker

Install Docker. Vagrant's Docker provider is built in.

### Check the provider

In an empty folder:

```bash
printf 'Vagrant.configure("2") { |c| c.vm.box = "bento/ubuntu-24.04" }\n' > Vagrantfile
VAGRANT_DEFAULT_PROVIDER=vmware_desktop vagrant status
```

Replace `vmware_desktop` with your provider (`virtualbox`, `parallels`, `libvirt`, `hyperv`, `docker`). It should say `not created`, not an error.

## 3. Install Boxdeck

Download the latest release from [GitHub Releases](https://github.com/oops-apps/boxdeck/releases):

| System  | File                            | First launch                                                                                                                                                                            |
| ------- | ------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| macOS   | `.dmg` (Apple Silicon or Intel) | Drag Boxdeck to Applications. Early builds aren't notarized, so macOS blocks the first launch: see [unsigned app warnings](troubleshooting.md#unsigned-app-warnings-macos-and-windows). |
| Windows | `.msi` or `-setup.exe`          | Early builds aren't signed. If SmartScreen warns, choose **More info → Run anyway**.                                                                                                    |
| Linux   | `.deb`, `.rpm` or `.AppImage`   | Install the package, or `chmod +x` the AppImage and run it. On GNOME, install the AppIndicator extension for the tray icon.                                                             |

When Boxdeck starts, it finds `vagrant` on your `PATH` and lists every machine Vagrant knows about. If it can't find Vagrant, it shows these steps; after installing, click **Look again**.

To use Boxdeck from AI agents, see [mcp.md](mcp.md).

## Building from source

See [Development](../README.md#development) in the README. You'll also need Rust, Node.js 22+, the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) and `cargo install cargo-deny`.
