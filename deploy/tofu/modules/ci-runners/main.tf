# Self-hosted, ephemeral GitHub Actions runners for this repository's CI (DEC-2065). Each droplet
# runs one job at a time: a root-owned loop mints a just-in-time runner configuration, which GitHub
# accepts for exactly one job, and starts a fresh Sysbox container from a baked image for it. The
# container runs the job, its own Docker daemon and any k3d cluster inside a user namespace, so the
# job is root only inside the container and never reaches the host's Docker socket. When the job
# ends the container is removed with everything it wrote, and the loop starts the next one.
#
# The only credential on a droplet is the GitHub App's private key, readable by root on the host
# alone and never mounted into a job's container. Every outbound port but DNS, NTP and ports 80 and 443
# is closed, nothing is admitted inbound, and the jobs cannot reach the host or the metadata
# service, which holds the boot script and so the key.

terraform {
  required_version = "1.13.1"

  required_providers {
    digitalocean = {
      source  = "digitalocean/digitalocean"
      version = "2.103.0"
    }
  }
}

variable "name_prefix" {
  description = "Names the droplets, their tag and their firewall."
  type        = string
  default     = "mandate-ci"
}

variable "region" {
  type = string
}

variable "size" {
  description = "The droplet size, one runner each."
  type        = string
}

variable "runner_count" {
  type = number

  validation {
    condition     = var.runner_count >= 1 && var.runner_count <= 10
    error_message = "Between 1 and 10 runners."
  }
}

variable "cap_usd" {
  description = "The runners' monthly cap, apart from DEC-2011's hosting budget (DEC-2065)."
  type        = number

  validation {
    condition     = var.cap_usd > 0 && var.cap_usd <= 150
    error_message = "The founder capped the runners at 150 USD a month (DEC-2065); a higher cap is his decision."
  }
}

variable "repository" {
  description = "The one repository the runners register with, as owner/name."
  type        = string

  validation {
    condition     = can(regex("^[A-Za-z0-9-]+/[A-Za-z0-9._-]+$", var.repository))
    error_message = "The repository is owner/name."
  }
}

variable "runner_label" {
  description = "The label only these runners carry, which ci.yml's CI_RUNNER variable names."
  type        = string

  validation {
    condition     = can(regex("^[a-z0-9-]+$", var.runner_label)) && !contains(["self-hosted", "linux", "x64", "ubuntu-24.04", "ubuntu-latest"], var.runner_label)
    error_message = "The label is lower-case letters, digits and dashes, and not a label GitHub or another runner also uses."
  }
}

variable "github_app_client_id" {
  description = "The GitHub App's client id, the issuer of the JSON web tokens the loop signs."
  type        = string

  validation {
    condition     = can(regex("^[A-Za-z0-9.]+$", var.github_app_client_id))
    error_message = "The client id is letters, digits and dots."
  }
}

variable "github_app_installation_id" {
  description = "The App's installation on the repository."
  type        = string

  validation {
    condition     = can(regex("^[0-9]+$", var.github_app_installation_id))
    error_message = "The installation id is a number."
  }
}

variable "github_app_private_key" {
  description = "The App's PEM private key. It reaches the droplets through their boot script; the provider keeps only a digest of that script in state."
  type        = string
  sensitive   = true

  validation {
    condition     = startswith(trimspace(var.github_app_private_key), "-----BEGIN") && endswith(trimspace(var.github_app_private_key), "PRIVATE KEY-----")
    error_message = "The private key is the PEM that GitHub downloads for the App, whole."
  }
}

variable "runner_version" {
  description = "The actions/runner release baked into the image; the runner updates itself when GitHub requires it."
  type        = string
}

variable "runner_sha256" {
  type = string
}

variable "sysbox_version" {
  type = string
}

variable "sysbox_sha256" {
  type = string
}

variable "base_image" {
  description = "The job image's base, pinned by digest."
  type        = string

  validation {
    condition     = can(regex("^ubuntu:24[.]04@sha256:[0-9a-f]{64}$", var.base_image))
    error_message = "The base image is ubuntu:24.04 pinned by digest."
  }
}

variable "toolchain_ref" {
  description = "The branch of the repository whose .cursor/install.sh the image runs, so jobs start with the pinned toolchain."
  type        = string
}

locals {
  repository_name = element(split("/", var.repository), 1)

  # The job image's build: the packages a GitHub-hosted ubuntu-24.04 job relies on, a Docker
  # daemon for the job's own containers, the runner, and the repository's pinned toolchain,
  # installed by its own install.sh as the runner user.
  image_build = <<-BUILD
    #!/bin/bash
    set -euo pipefail
    export DEBIAN_FRONTEND=noninteractive
    apt-get update
    apt-get upgrade -y
    apt-get install -y --no-install-recommends ca-certificates curl git jq sudo unzip zip xz-utils zstd build-essential pkg-config libssl-dev python3 python3-venv docker.io iptables iproute2 gnupg lsb-release tzdata locales openssh-client
    useradd --create-home --uid 1001 --shell /bin/bash runner
    usermod -aG docker runner
    echo 'runner ALL=(ALL) NOPASSWD: ALL' > /etc/sudoers.d/runner
    chmod 0440 /etc/sudoers.d/runner
    mkdir -p /home/runner/actions-runner
    cd /home/runner/actions-runner
    curl -fsSL --retry 5 --retry-all-errors -o runner.tgz "https://github.com/actions/runner/releases/download/v${var.runner_version}/actions-runner-linux-x64-${var.runner_version}.tar.gz"
    echo "${var.runner_sha256}  runner.tgz" | sha256sum --check
    tar xzf runner.tgz
    rm runner.tgz
    ./bin/installdependencies.sh
    chown -R runner:runner /home/runner
    sudo -u runner -H bash /opt/toolchain.sh
    rm -rf /var/lib/apt/lists/* /tmp/*
  BUILD

  # Runs as the runner user inside the image's build. install.sh is best effort here: a job
  # installs anything still missing itself, as it does on a GitHub-hosted runner.
  image_toolchain = <<-TOOLCHAIN
    #!/bin/bash
    set -euo pipefail
    cd "$HOME"
    curl -fsSL --retry 5 --retry-all-errors -o rustup-init https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init
    curl -fsSL --retry 5 --retry-all-errors -o rustup-init.sha256 https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init.sha256
    echo "$(cut -d ' ' -f 1 rustup-init.sha256)  rustup-init" | sha256sum --check
    chmod +x rustup-init
    ./rustup-init -y --no-modify-path --default-toolchain none --profile minimal
    rm rustup-init rustup-init.sha256
    export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
    git clone --depth 1 --branch "${var.toolchain_ref}" "https://github.com/${var.repository}.git" /tmp/mandate
    cd /tmp/mandate
    bash .cursor/install.sh || echo "install.sh did not finish; jobs install what is missing"
    cd "$HOME"
    rm -rf /tmp/mandate
  TOOLCHAIN

  # The container's first process: its own Docker daemon, then the runner as the runner user with
  # the one-job configuration the host's loop passed in.
  image_entry = <<-ENTRY
    #!/bin/bash
    set -euo pipefail
    dockerd > /var/log/dockerd.log 2>&1 &
    for _ in $(seq 1 60); do
      if docker info > /dev/null 2>&1; then
        break
      fi
      sleep 1
    done
    exec sudo -u runner -H env ACTIONS_RUNNER_HOOK_JOB_STARTED=/usr/local/bin/mandate-runner-admit.sh PATH=/home/runner/.cargo/bin:/home/runner/.local/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin /home/runner/actions-runner/run.sh --jitconfig "$1"
  ENTRY

  # The runner's job-started hook, which runs before any step of every job and fails the job
  # unless it is this repository's own: a pull request whose head is a branch of it, or a push or
  # dispatch on main. ci.yml's guard keeps honest runs off these runners, but a fork's pull request
  # brings its own workflow, which can name the label directly; this hook is part of the image, so
  # no workflow can change it (DEC-2065 item 4). Anything it cannot read is refused.
  image_admit = <<-ADMIT
    #!/bin/bash
    set -eo pipefail
    refuse() {
      echo "This runner takes only ${var.repository}'s own jobs: $1 (DEC-2065)." >&2
      exit 1
    }
    [ "$GITHUB_REPOSITORY" = "${var.repository}" ] || refuse "a job of $GITHUB_REPOSITORY"
    [ -r "$GITHUB_EVENT_PATH" ] || refuse "no event to read"
    case "$GITHUB_EVENT_NAME" in
      pull_request)
        head=$(jq -r '.pull_request.head.repo.full_name // ""' "$GITHUB_EVENT_PATH")
        [ "$head" = "${var.repository}" ] || refuse "a pull request from $head"
        ;;
      push | workflow_dispatch)
        [ "$GITHUB_REF" = refs/heads/main ] || refuse "a $GITHUB_EVENT_NAME on $GITHUB_REF"
        ;;
      *)
        refuse "a $GITHUB_EVENT_NAME event"
        ;;
    esac
  ADMIT

  image_dockerfile = <<-DOCKER
    FROM ${var.base_image}
    COPY build.sh toolchain.sh /opt/
    COPY entry.sh /usr/local/bin/mandate-runner-entry
    COPY admit.sh /usr/local/bin/mandate-runner-admit.sh
    RUN chmod 0755 /usr/local/bin/mandate-runner-entry /usr/local/bin/mandate-runner-admit.sh && bash /opt/build.sh && rm /opt/build.sh /opt/toolchain.sh
  DOCKER

  # The host's loop, one job per run; systemd starts it again when it exits, however it exits. It
  # registers a runner only while the repository holds every outside collaborator's fork run for
  # approval, read with the App's token, and fails closed: a setting it cannot read registers none.
  manager = <<-MANAGER
    #!/bin/bash
    set -euo pipefail
    . /etc/mandate-runner/env
    api=https://api.github.com
    image=mandate-ci-runner:current
    b64url() {
      openssl base64 -A | tr '+/' '-_' | tr -d '='
    }
    app_jwt() {
      now=$(date +%s)
      header=$(printf '%s' '{"alg":"RS256","typ":"JWT"}' | b64url)
      claims=$(jq -cjn --arg iss "$APP_CLIENT_ID" --argjson iat "$((now - 60))" --argjson exp "$((now + 540))" '{iat: $iat, exp: $exp, iss: $iss}' | b64url)
      signature=$(printf '%s.%s' "$header" "$claims" | openssl dgst -sha256 -sign /etc/mandate-runner/app.pem -binary | b64url)
      printf '%s.%s.%s' "$header" "$claims" "$signature"
    }
    built=$(docker image inspect --format '{{.Created}}' "$image" 2> /dev/null || echo 1970-01-01T00:00:00Z)
    if [ "$(( $(date +%s) - $(date -d "$built" +%s) ))" -gt 86400 ]; then
      if docker build --no-cache --pull -t "$image" /opt/mandate-runner/image; then
        docker image prune --force
      elif [ "$built" = 1970-01-01T00:00:00Z ]; then
        exit 1
      else
        echo "the daily rebuild failed; keeping the image built at $built" >&2
      fi
    fi
    if [ -n "$(docker ps --quiet --filter 'name=^mandate-ci-job$')" ]; then
      docker wait mandate-ci-job
    fi
    docker container prune --force
    token=$(curl -fsS --retry 3 -X POST -H "Authorization: Bearer $(app_jwt)" -H "Accept: application/vnd.github+json" "$api/app/installations/$INSTALLATION_ID/access_tokens" -d "$(jq -cn --arg r "$REPOSITORY_NAME" '{repositories: [$r], permissions: {administration: "write"}}')" | jq -er .token)
    policy=$(curl -fsS --retry 3 -H "Authorization: Bearer $token" -H "Accept: application/vnd.github+json" "$api/repos/$REPOSITORY/actions/permissions/fork-pr-contributor-approval" | jq -r '.approval_policy // ""' || true)
    if [ "$policy" != all_external_contributors ]; then
      curl -fsS -X DELETE -H "Authorization: Bearer $token" -H "Accept: application/vnd.github+json" "$api/installation/token" || true
      echo "registering no runner: fork pull requests from outside collaborators are not all held for approval (DEC-2065 item 4)" >&2
      sleep 300
      exit 1
    fi
    name="$(hostname)-$(date +%s)"
    config=$(curl -fsS --retry 3 -X POST -H "Authorization: Bearer $token" -H "Accept: application/vnd.github+json" "$api/repos/$REPOSITORY/actions/runners/generate-jitconfig" -d "$(jq -cn --arg n "$name" --arg l "$LABEL" '{name: $n, runner_group_id: 1, labels: ["self-hosted", "linux", "x64", $l], work_folder: "_work"}')" | jq -er .encoded_jit_config)
    curl -fsS -X DELETE -H "Authorization: Bearer $token" -H "Accept: application/vnd.github+json" "$api/installation/token" || true
    unset token
    exec docker run --rm --runtime=sysbox-runc --name mandate-ci-job --hostname "$name" "$image" mandate-runner-entry "$config"
  MANAGER

  # Keeps jobs off the host, the metadata service and every private range, so a job reaches no
  # other droplet or cluster in the account's VPCs. Docker keeps DOCKER-USER across its own
  # restarts, and the unit runs again with every Docker start.
  fence = <<-FENCE
    #!/bin/bash
    set -euo pipefail
    for range in 169.254.0.0/16 10.0.0.0/8 172.16.0.0/12 192.168.0.0/16 100.64.0.0/10; do
      iptables -C DOCKER-USER -i docker0 -d "$range" -j REJECT 2> /dev/null || iptables -I DOCKER-USER -i docker0 -d "$range" -j REJECT
    done
    iptables -C INPUT -i docker0 -j REJECT 2> /dev/null || iptables -I INPUT -i docker0 -j REJECT
  FENCE

  boot = <<-BOOT
    #!/bin/bash
    # A CI runner host (DEC-2065), made by deploy/tofu/modules/ci-runners.
    set -euo pipefail
    export DEBIAN_FRONTEND=noninteractive
    passwd --lock root
    install -d -m 0700 /etc/mandate-runner
    umask 077
    cat > /etc/mandate-runner/app.pem <<'PEM'
    ${trimspace(var.github_app_private_key)}
    PEM
    cat > /etc/mandate-runner/env <<'ENV'
    APP_CLIENT_ID=${var.github_app_client_id}
    INSTALLATION_ID=${var.github_app_installation_id}
    REPOSITORY=${var.repository}
    REPOSITORY_NAME=${local.repository_name}
    LABEL=${var.runner_label}
    ENV
    umask 022
    apt-get -o DPkg::Lock::Timeout=600 update
    apt-get -o DPkg::Lock::Timeout=600 install -y --no-install-recommends docker.io jq curl ca-certificates openssl
    install -d -m 0755 /etc/docker
    echo '{}' > /etc/docker/daemon.json
    curl -fsSL --retry 5 --retry-all-errors -o /root/sysbox.deb "https://github.com/nestybox/sysbox/releases/download/v${var.sysbox_version}/sysbox-ce_${var.sysbox_version}.linux_amd64.deb"
    echo "${var.sysbox_sha256}  /root/sysbox.deb" | sha256sum --check
    apt-get -o DPkg::Lock::Timeout=600 install -y /root/sysbox.deb
    rm /root/sysbox.deb
    jq '. + {"bip": "10.213.0.1/24", "default-address-pools": [{"base": "10.214.0.0/16", "size": 24}]}' /etc/docker/daemon.json > /etc/docker/daemon.json.new
    mv /etc/docker/daemon.json.new /etc/docker/daemon.json
    systemctl restart docker
    docker info --format '{{json .Runtimes}}' | grep -q sysbox-runc
    install -d -m 0755 /opt/mandate-runner/image
    base64 -d > /opt/mandate-runner/image/Dockerfile <<'B64'
    ${base64encode(local.image_dockerfile)}
    B64
    base64 -d > /opt/mandate-runner/image/build.sh <<'B64'
    ${base64encode(local.image_build)}
    B64
    base64 -d > /opt/mandate-runner/image/toolchain.sh <<'B64'
    ${base64encode(local.image_toolchain)}
    B64
    base64 -d > /opt/mandate-runner/image/entry.sh <<'B64'
    ${base64encode(local.image_entry)}
    B64
    base64 -d > /opt/mandate-runner/image/admit.sh <<'B64'
    ${base64encode(local.image_admit)}
    B64
    base64 -d > /usr/local/sbin/mandate-runner-manager <<'B64'
    ${base64encode(local.manager)}
    B64
    base64 -d > /usr/local/sbin/mandate-runner-fence <<'B64'
    ${base64encode(local.fence)}
    B64
    chmod 0700 /usr/local/sbin/mandate-runner-manager /usr/local/sbin/mandate-runner-fence
    cat > /etc/systemd/system/mandate-runner-fence.service <<'UNIT'
    [Unit]
    Description=Keep CI jobs off the host and the metadata service (DEC-2065)
    After=docker.service
    PartOf=docker.service
    [Service]
    Type=oneshot
    RemainAfterExit=yes
    ExecStart=/usr/local/sbin/mandate-runner-fence
    [Install]
    WantedBy=docker.service
    UNIT
    cat > /etc/systemd/system/mandate-runner.service <<'UNIT'
    [Unit]
    Description=One ephemeral GitHub Actions job at a time (DEC-2065)
    Wants=network-online.target
    After=network-online.target docker.service sysbox.service mandate-runner-fence.service
    Requires=docker.service mandate-runner-fence.service
    [Service]
    ExecStart=/usr/local/sbin/mandate-runner-manager
    Restart=always
    RestartSec=30
    [Install]
    WantedBy=multi-user.target
    UNIT
    systemctl daemon-reload
    systemctl enable --now mandate-runner-fence.service
    systemctl enable --now mandate-runner.service
  BOOT
}

module "budget" {
  source             = "../budget"
  node_size          = var.size
  region             = var.region
  node_count         = var.runner_count
  spaces_monthly_usd = 0
  block_storage_gb   = 0
  cap_usd            = var.cap_usd
  cap_reason         = "DEC-2065"
}

resource "digitalocean_tag" "runner" {
  name = "${var.name_prefix}-runner"
}

resource "digitalocean_firewall" "runner" {
  name = "${var.name_prefix}-runner"
  tags = [digitalocean_tag.runner.id]

  outbound_rule {
    protocol              = "tcp"
    port_range            = "443"
    destination_addresses = ["0.0.0.0/0"]
  }

  outbound_rule {
    protocol              = "tcp"
    port_range            = "80"
    destination_addresses = ["0.0.0.0/0"]
  }

  outbound_rule {
    protocol              = "udp"
    port_range            = "53"
    destination_addresses = ["0.0.0.0/0"]
  }

  outbound_rule {
    protocol              = "tcp"
    port_range            = "53"
    destination_addresses = ["0.0.0.0/0"]
  }

  outbound_rule {
    protocol              = "udp"
    port_range            = "123"
    destination_addresses = ["0.0.0.0/0"]
  }
}

resource "digitalocean_droplet" "runner" {
  count             = var.runner_count
  name              = "${var.name_prefix}-runner-${count.index + 1}"
  region            = var.region
  size              = var.size
  image             = "ubuntu-24-04-x64"
  ipv6              = false
  monitoring        = false
  backups           = false
  droplet_agent     = false
  graceful_shutdown = false
  tags              = [digitalocean_tag.runner.id]
  user_data         = local.boot
}

output "estimate_usd" {
  value = module.budget.estimate_usd
}

output "runner_names" {
  value = digitalocean_droplet.runner[*].name
}
