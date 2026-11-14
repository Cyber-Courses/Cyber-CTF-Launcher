//! The command line's arguments: every command and option, with its help.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Cyber CTF from the command line: the launcher's labs, attack boxes, machine setup, servers,
/// cloud accounts and account, through the same code as the app, with or without the app open.
#[derive(Parser, Debug)]
#[command(name = "cyberctf", version, disable_help_subcommand = true)]
pub struct Cli {
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
    /// Print one JSON object a line (events, then the result or the error), for scripts and tests.
    #[arg(long, global = true)]
    pub json: bool,
    /// Answer yes to every confirmation; needed off a terminal for what changes or deletes things,
    /// and there it never shows a prompt (an install then elevates unattended).
    #[arg(long, short = 'y', global = true)]
    pub yes: bool,
    /// The launcher's data folder (default: the app's own; `CYBERCTF_DATA_DIR` too).
    #[arg(long, global = true, env = "CYBERCTF_DATA_DIR", value_name = "DIR")]
    pub data_dir: Option<PathBuf>,
    /// Log what the launcher does to stderr.
    #[arg(long, short = 'v', global = true)]
    pub verbose: bool,
}

/// The commands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// The labs installed here, and what can be done with them.
    #[command(subcommand)]
    Labs(Labs),
    /// The attack box of a running lab.
    #[command(subcommand, name = "attack-box")]
    AttackBox(AttackBox),
    /// This machine: its check, the guided setup and the tools it installs.
    #[command(subcommand)]
    Machine(Machine),
    /// The saved servers and cloud accounts.
    #[command(subcommand)]
    Servers(Servers),
    /// The cloud CLIs' sign-ins and what they see.
    #[command(subcommand)]
    Cloud(Cloud),
    /// The Cyber CTF account.
    #[command(subcommand)]
    Account(Account),
    /// What Cyber CTF stored on this machine.
    #[command(subcommand)]
    Storage(Storage),
    /// One report of what the app's health screens show: the machine, the account, the servers,
    /// the labs and the operations in flight.
    Doctor,
    /// This launcher's version.
    Version,
    /// The shell completion script for `shell` (bash, zsh, fish, powershell, elvish), on stdout.
    Completions {
        /// The shell.
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

/// How a lab runs, as its catalogue entry declares it.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeArg {
    /// In containers.
    Docker,
    /// In virtual machines.
    Vm,
    /// As cloud services.
    Cloud,
}

/// A lab, and how it runs when its installed spec can't say.
#[derive(Args, Debug)]
pub struct LabArg {
    /// The lab's id.
    pub id: String,
    /// How the lab runs (default: what its installed spec declares).
    #[arg(long, value_enum)]
    pub runtime: Option<RuntimeArg>,
}

/// The lab commands.
#[derive(Subcommand, Debug)]
pub enum Labs {
    /// The labs installed here: whether each runs, and any operation on it.
    List,
    /// What the lab's details card shows: where it runs, its machines and ports, its credentials.
    Show {
        /// The lab.
        #[command(flatten)]
        lab: LabArg,
        /// Show the passwords and secret values instead of masking them.
        #[arg(long)]
        show_secrets: bool,
    },
    /// Imports the lab in a folder (an unpublished lab) and starts it.
    Install {
        /// The lab's folder (with its isoloom.yml).
        folder: PathBuf,
    },
    /// Starts a lab of the catalogue (signed in), downloading it first.
    Start(StartArgs),
    /// Stops the lab wherever it runs and removes what it ran.
    Stop {
        /// The lab.
        #[command(flatten)]
        lab: LabArg,
    },
    /// Pauses the lab (state saved) or shuts it down, keeping its machines.
    Park {
        /// The lab.
        #[command(flatten)]
        lab: LabArg,
        /// Shut the machines down instead of pausing them.
        #[arg(long)]
        shutdown: bool,
    },
    /// Brings a parked lab back.
    Resume {
        /// The lab.
        #[command(flatten)]
        lab: LabArg,
    },
    /// Removes the lab from this computer, with everything the app keeps for it.
    Remove {
        /// The lab's id.
        id: String,
    },
    /// Whether the lab runs, where, and its machines' states.
    Status {
        /// The lab.
        #[command(flatten)]
        lab: LabArg,
    },
    /// The lab's operations, as the runner service journals them.
    Logs {
        /// The lab's id.
        id: String,
        /// Follow its operations under way to their end.
        #[arg(long, short = 'f')]
        follow: bool,
    },
}

/// Where and how a catalogue lab starts.
#[derive(Args, Debug)]
pub struct StartArgs {
    /// The lab's id in the catalogue.
    pub id: String,
    /// The hypervisor or cloud to run it on (`virtualbox`, `libvirt`, `aws`, …).
    #[arg(long)]
    pub provider: Option<String>,
    /// The saved server or cloud account to run it on, by id.
    #[arg(long)]
    pub host: Option<String>,
    /// Publish the lab's ports on their own numbers.
    #[arg(long)]
    pub default_ports: bool,
    /// Start the installed copy without asking Cyber CTF.
    #[arg(long)]
    pub offline: bool,
    /// Run a container lab as one VM per machine.
    #[arg(long)]
    pub per_machine_vm: bool,
    /// The Docker engine (context) to run a container lab on.
    #[arg(long)]
    pub docker_engine: Option<String>,
}

/// The attack box commands.
#[derive(Subcommand, Debug)]
pub enum AttackBox {
    /// Adds the attack box to the running lab.
    Start {
        /// The lab.
        #[command(flatten)]
        lab: LabArg,
        /// The lab network it joins (default: the one the lab declares, else its first).
        #[arg(long)]
        network: Option<String>,
        /// The container image of a container box (default: the launcher's Kali).
        #[arg(long)]
        image: Option<String>,
        /// A VM box with Kali's desktop.
        #[arg(long)]
        desktop: bool,
    },
    /// Takes the attack box out of the running lab.
    Stop {
        /// The lab.
        #[command(flatten)]
        lab: LabArg,
    },
    /// The attack box's state, its address and how to sign in to it.
    Status {
        /// The lab.
        #[command(flatten)]
        lab: LabArg,
        /// Show its password instead of masking it.
        #[arg(long)]
        show_secrets: bool,
    },
}

/// The lab types the guided setup readies this machine for.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupFor {
    /// Container labs: a Docker engine (and WSL 2 on Windows).
    Docker,
    /// VM labs: a hypervisor and Vagrant with what it needs for it.
    Vm,
}

/// The machine commands.
#[derive(Subcommand, Debug)]
pub enum Machine {
    /// What this machine has installed and can run (the Machine page's check).
    Check,
    /// The guided setup: the plan for this machine, then each step run as the setup window runs
    /// it, the machine checked again after each.
    Setup {
        /// The lab types to ready this machine for (default: both).
        #[arg(long = "for", value_enum, value_delimiter = ',')]
        paths: Vec<SetupFor>,
        /// The hypervisors to set up for VM labs, in order of preference (`libvirt`,
        /// `virtualbox`, …): the first the setup can set up is the one.
        #[arg(long = "provider", value_delimiter = ',')]
        providers: Vec<String>,
        /// The Docker engine to set up (`docker-engine`, `docker-desktop`, `colima`, …).
        #[arg(long)]
        engine: Option<String>,
        /// Print the plan, run nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// The tools Cyber CTF installed, or installs and removes one.
    #[command(subcommand)]
    Tools(Tools),
}

/// The tool commands.
#[derive(Subcommand, Debug)]
pub enum Tools {
    /// The tools Cyber CTF installed (which a removal may remove).
    List,
    /// Installs a tool (`docker`, `vagrant`, `virtualbox`, `libvirt`, `qemu`, `terraform`, …), or a
    /// Vagrant plugin as `plugin:<name>`.
    Install {
        /// The tool.
        tool: String,
    },
    /// Removes a tool Cyber CTF installed.
    Remove {
        /// The tool.
        tool: String,
    },
}

/// The server commands.
#[derive(Subcommand, Debug)]
pub enum Servers {
    /// The saved servers and cloud accounts, the default marked.
    List,
    /// One saved server or cloud account.
    Show {
        /// Its id.
        id: String,
    },
    /// Tests the connection to a saved server or cloud account.
    Test {
        /// Its id.
        id: String,
    },
    /// Removes a saved server or cloud account no lab runs on.
    Remove {
        /// Its id.
        id: String,
    },
    /// Makes a saved server the one VM labs run on by default (`--none`: none).
    SetDefault {
        /// Its id.
        id: Option<String>,
        /// Clear the default.
        #[arg(long, conflicts_with = "id")]
        none: bool,
    },
    /// The launcher's SSH public key, which a Proxmox server authorizes.
    PublicKey,
}

/// A cloud whose CLI the launcher signs in with.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloudArg {
    /// AWS (`aws login`).
    Aws,
    /// Azure (`az login`).
    Azure,
    /// Google Cloud (`gcloud auth login`).
    Gcp,
}

/// The cloud commands.
#[derive(Subcommand, Debug)]
pub enum Cloud {
    /// What each cloud CLI is signed in as.
    Status,
    /// Signs in to a cloud through its CLI, in the browser.
    Signin {
        /// The cloud.
        #[arg(value_enum)]
        cloud: CloudArg,
        /// The AWS profile to sign in.
        #[arg(long)]
        profile: Option<String>,
    },
}

/// The account commands.
#[derive(Subcommand, Debug)]
pub enum Account {
    /// Whether this launcher is signed in, and as whom.
    Status,
    /// Signs in with a code entered on any device (or `--browser`: through a sign-in link).
    Signin {
        /// Sign in through a link opened in a browser on this machine.
        #[arg(long)]
        browser: bool,
    },
    /// Signs out.
    Signout,
}

/// The storage commands.
#[derive(Subcommand, Debug)]
pub enum Storage {
    /// What Cyber CTF's labs and images take on disk.
    Show,
    /// Removes what Cyber CTF stored that no lab needs now.
    Clean,
}
