#!/usr/bin/env bash
# Records real Vagrant output into crates/boxdeck-core/tests/fixtures/captured/<vagrant-version>-<os>/.
#
# Default mode uses a throwaway VAGRANT_HOME and offline fake boxes, so it never touches your
# real Vagrant data and needs no network or running VMs.
# `--with-docker` uses Vagrant's built-in Docker provider to create (then destroy) a tiny
# Alpine container, to capture real running/stopped states and machine-index entries.
# `--with-vmware` creates one small VMware VM (box bento/ubuntu-24.04) in a temporary project,
# walks it through up/suspend/resume/halt/destroy, and records each state. It uses your normal
# Vagrant home, because that's where the vagrant-vmware-desktop plugin lives; the box stays
# installed afterwards. Needs the Vagrant VMware Utility.
# `--project DIR` additionally captures status/port/ssh-config of one of your real projects.
# All output is scrubbed of your home directory, user name and host name before it is saved.
set -euo pipefail

usage() { echo "usage: $0 [--with-docker] [--with-vmware] [--project DIR]" >&2; exit 2; }

project=""
docker=0
vmware=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --project) project="${2:?}"; shift 2 ;;
    --with-docker) docker=1; shift ;;
    --with-vmware) vmware=1; shift ;;
    *) usage ;;
  esac
done

command -v vagrant >/dev/null || { echo "vagrant not found on PATH" >&2; exit 1; }

repo="$(cd "$(dirname "$0")/.." && pwd)"
version="$(vagrant --version | awk '{print $2}')"
os="$(uname -s | tr '[:upper:]' '[:lower:]')"
out="$repo/crates/boxdeck-core/tests/fixtures/captured/${version}-${os}"
mkdir -p "$out"

# Test projects live in the repo's gitignored .testbed/ so they're easy to find and inspect.
mkdir -p "$repo/.testbed"
work="$(cd "$(mktemp -d "$repo/.testbed/capture.XXXXXX")" && pwd -P)"
trap 'rm -rf "$work"' EXIT
export VAGRANT_HOME="$work/home"
export VAGRANT_CHECKPOINT_DISABLE=1
export VAGRANT_NO_COLOR=1

scrub() {
  # Order matters: the temp dir lives under $HOME on some systems.
  sed -e "s|$work|/tmp/boxdeck-capture|g" \
      -e "s|$HOME|/home/user|g" \
      -e "s|$(id -un)|user|g" \
      -e "s|$(hostname -s)|host|g"
}

# capture NAME DIR ARGS...: runs vagrant in DIR, saves stdout, stderr and the exit code.
capture() {
  local name="$1" dir="$2"; shift 2
  local code=0
  (cd "$dir" && vagrant "$@") >"$work/stdout" 2>"$work/stderr" || code=$?
  scrub <"$work/stdout" >"$out/$name.stdout"
  scrub <"$work/stderr" >"$out/$name.stderr"
  echo "$code" >"$out/$name.exit"
  printf '  %-34s exit %s\n' "$name" "$code"
}

# make_box FILE PROVIDER [ARCH]: a minimal offline box (metadata.json only).
make_box() {
  local file="$1" provider="$2" arch="${3:-}" dir
  dir="$(mktemp -d "$work/box.XXXX")"
  if [[ -n "$arch" ]]; then
    printf '{"provider":"%s","architecture":"%s"}' "$provider" "$arch" >"$dir/metadata.json"
  else
    printf '{"provider":"%s"}' "$provider" >"$dir/metadata.json"
  fi
  printf '{"author":"boxdeck","homepage":"https://example.com/a,b"}' >"$dir/info.json"
  tar -czf "$file" -C "$dir" .
}

echo "Capturing Vagrant $version on $os into $out"

mkdir -p "$work/single" "$work/broken"
cat >"$work/single/Vagrantfile" <<'RUBY'
Vagrant.configure("2") do |config|
  config.vm.box = "boxdeck/test"
end
RUBY
printf 'Vagrant.configure("2") do |config|\n  config.vm.box = \n' >"$work/broken/Vagrantfile"

capture version "$work/single" --version
capture box-list-empty "$work/single" box list --machine-readable
capture global-status-empty "$work/single" global-status --machine-readable

make_box "$work/plain.box" vmware_desktop
make_box "$work/arm.box" vmware_desktop arm64
make_box "$work/vbox.box" virtualbox amd64
(cd "$work/single" && vagrant box add --name boxdeck/test "$work/plain.box" >/dev/null)
(cd "$work/single" && vagrant box add --name boxdeck/arch --architecture arm64 "$work/arm.box" >/dev/null)
(cd "$work/single" && vagrant box add --name boxdeck/arch --architecture amd64 "$work/vbox.box" >/dev/null)

capture box-list "$work/single" box list --machine-readable
capture box-list-info "$work/single" box list -i --machine-readable
# No usable provider is configured here, so this records the error-exit path.
capture status-no-provider "$work/single" status --machine-readable
capture status-syntax-error "$work/broken" status --machine-readable
capture validate-syntax-error "$work/broken" validate
capture box-prune-dry-run "$work/single" box prune -n
capture box-outdated-global "$work/single" box outdated --global
capture box-outdated-global-mr "$work/single" box outdated --global --machine-readable
# Needs internet; anonymous search of the public box catalog.
capture cloud-search "$work/single" cloud search --json --no-auth --provider vmware_desktop --architecture arm64 --limit 5 ubuntu
# Why the search never gets --no-tty: it turns the flags into part of the query.
capture cloud-search-no-tty "$work/single" cloud search --no-tty ubuntu --limit 3

if [[ "$docker" == 1 ]]; then
  command -v docker >/dev/null || { echo "docker not found on PATH" >&2; exit 1; }
  export VAGRANT_DEFAULT_PROVIDER=docker
  mkdir -p "$work/dmulti" "$work/dcustom"
  cat >"$work/dmulti/Vagrantfile" <<'RUBY'
Vagrant.configure("2") do |config|
  config.vm.provider "docker" do |d|
    d.image = "alpine:3"
    d.cmd = ["sleep", "infinity"]
    d.has_ssh = false
  end
  config.vm.define "web"
  config.vm.define "db"
end
RUBY
  sed -e 's/"web"/"custom"/' -e '/"db"/d' "$work/dmulti/Vagrantfile" >"$work/dcustom/Custom.rb"

  capture status-multi-not-created "$work/dmulti" status --machine-readable
  capture status-unknown-machine "$work/dmulti" status nope --machine-readable
  capture status-no-tty-overrides "$work/dmulti" status --machine-readable --no-tty
  capture validate-ok "$work/dmulti" validate
  capture status-custom-vagrantfile-missing "$work/dcustom" status --machine-readable
  VAGRANT_VAGRANTFILE=Custom.rb capture status-vagrantfile-env "$work/dcustom" status --machine-readable

  capture up-web "$work/dmulti" up web --no-tty
  capture status-multi-running "$work/dmulti" status --machine-readable
  capture status-multi-one "$work/dmulti" status web --machine-readable
  capture destroy-prompt-no-tty "$work/dmulti" destroy web --no-tty
  capture halt-web "$work/dmulti" halt web --no-tty
  capture status-multi-stopped "$work/dmulti" status --machine-readable
  VAGRANT_VAGRANTFILE=Custom.rb capture up-custom "$work/dcustom" up --no-tty
  scrub <"$VAGRANT_HOME/data/machine-index/index" >"$out/machine-index.json"
  capture global-status "$work/single" global-status --machine-readable
  capture destroy-web "$work/dmulti" destroy -f web --no-tty
  VAGRANT_VAGRANTFILE=Custom.rb capture destroy-custom "$work/dcustom" destroy -f --no-tty
  scrub <"$VAGRANT_HOME/data/machine-index/index" >"$out/machine-index-after-destroy.json"
  unset VAGRANT_DEFAULT_PROVIDER
fi

if [[ "$vmware" == 1 ]]; then
  echo "Capturing VMware (downloads bento/ubuntu-24.04 on first run; takes a few minutes)"
  saved_home="$VAGRANT_HOME"
  unset VAGRANT_HOME
  export VAGRANT_DEFAULT_PROVIDER=vmware_desktop
  mkdir -p "$work/vmw"
  cat >"$work/vmw/Vagrantfile" <<'RUBY'
Vagrant.configure("2") do |config|
  config.vm.box = "bento/ubuntu-24.04"
  config.vm.synced_folder ".", "/vagrant", disabled: true
  # No host port forwards: some machines (VPN or security software) make every localhost port
  # look taken, which fails Vagrant's collision check. SSH goes to the VM's own IP instead.
  config.vm.network "forwarded_port", guest: 22, host: 2222, id: "ssh", disabled: true
  config.vm.define "web"
  config.vm.provider "vmware_desktop" do |v|
    v.gui = false
    v.vmx["memsize"] = "1024"
    v.vmx["numvcpus"] = "1"
  end
end
RUBY
  capture vmware-status-not-created "$work/vmw" status --machine-readable
  capture vmware-up "$work/vmw" up --no-tty
  capture vmware-status-running "$work/vmw" status --machine-readable
  capture vmware-port "$work/vmw" port web --machine-readable
  capture vmware-ssh-config "$work/vmw" ssh-config web --no-tty
  capture vmware-ssh-exec "$work/vmw" ssh web -c "uname -m"
  capture vmware-suspend "$work/vmw" suspend --no-tty
  capture vmware-status-saved "$work/vmw" status --machine-readable
  capture vmware-resume "$work/vmw" resume --no-tty
  capture vmware-halt "$work/vmw" halt --no-tty
  capture vmware-status-poweroff "$work/vmw" status --machine-readable
  capture vmware-box-list "$work/vmw" box list -i --machine-readable
  capture vmware-box-outdated "$work/vmw" box outdated --global --machine-readable
  capture vmware-destroy "$work/vmw" destroy -f --no-tty
  capture vmware-status-destroyed "$work/vmw" status --machine-readable
  unset VAGRANT_DEFAULT_PROVIDER
  export VAGRANT_HOME="$saved_home"
fi

if [[ -n "$project" ]]; then
  echo "Capturing your project (scrubbed): $project"
  unset VAGRANT_HOME
  capture project-status "$project" status --machine-readable
  capture project-port "$project" port --machine-readable
  capture project-ssh-config "$project" ssh-config --machine-readable
fi

echo "Done. Review the files before committing; anything personal left in them is a scrub bug."
