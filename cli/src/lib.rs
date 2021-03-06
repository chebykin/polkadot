// Copyright 2017 Parity Technologies (UK) Ltd.
// This file is part of Polkadot.

// Polkadot is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Polkadot is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Polkadot.  If not, see <http://www.gnu.org/licenses/>.

//! Polkadot CLI library.

#![warn(missing_docs)]
#![warn(unused_extern_crates)]

mod chain_spec;
#[cfg(feature = "browser")]
mod browser;

use chain_spec::ChainSpec;
use futures::{
	Future, FutureExt, TryFutureExt, future::select, channel::oneshot, compat::Future01CompatExt,
	task::Spawn
};
use tokio::runtime::Runtime;
use log::{info, error};
use structopt::StructOpt;

pub use service::{
	AbstractService, CustomConfiguration,
	ProvideRuntimeApi, CoreApi, ParachainHost,
	WrappedExecutor
};

pub use cli::{VersionInfo, IntoExit, NoCustom};
pub use cli::{display_role, error};

fn load_spec(id: &str) -> Result<Option<service::ChainSpec>, String> {
	Ok(match ChainSpec::from(id) {
		Some(spec) => Some(spec.load()?),
		None => None,
	})
}

/// Additional worker making use of the node, to run asynchronously before shutdown.
///
/// This will be invoked with the service and spawn a future that resolves
/// when complete.
pub trait Worker: IntoExit {
	/// A future that resolves when the work is done or the node should exit.
	/// This will be run on a tokio runtime.
	type Work: Future<Output=()> + Unpin + Send + 'static;

	/// Return configuration for the polkadot node.
	// TODO: make this the full configuration, so embedded nodes don't need
	// string CLI args (https://github.com/paritytech/polkadot/issues/111)
	fn configuration(&self) -> service::CustomConfiguration { Default::default() }

	/// Do work and schedule exit.
	fn work<S, SC, B, CE, SP>(self, service: &S, spawner: SP) -> Self::Work
	where S: AbstractService<Block = service::Block, RuntimeApi = service::RuntimeApi,
		Backend = B, SelectChain = SC,
		NetworkSpecialization = service::PolkadotProtocol, CallExecutor = CE>,
		SC: service::SelectChain<service::Block> + 'static,
		B: service::Backend<service::Block, service::Blake2Hasher> + 'static,
		CE: service::CallExecutor<service::Block, service::Blake2Hasher> + Clone + Send + Sync + 'static,
		SP: Spawn + Clone + Send + Sync + 'static;
}

#[derive(Debug, StructOpt, Clone)]
enum PolkadotSubCommands {
	#[structopt(name = "validation-worker", setting = structopt::clap::AppSettings::Hidden)]
	ValidationWorker(ValidationWorkerCommand),
}

impl cli::GetSharedParams for PolkadotSubCommands {
	fn shared_params(&self) -> Option<&cli::SharedParams> { None }
}

#[derive(Debug, StructOpt, Clone)]
struct ValidationWorkerCommand {
	#[structopt()]
	pub mem_id: String,
}

#[derive(Debug, StructOpt, Clone)]
struct PolkadotSubParams {
	#[structopt(long = "enable-authority-discovery")]
	pub authority_discovery_enabled: bool,
}

cli::impl_augment_clap!(PolkadotSubParams);

/// Parses polkadot specific CLI arguments and run the service.
pub fn run<W>(worker: W, version: cli::VersionInfo) -> error::Result<()> where
	W: Worker,
{
	match cli::parse_and_prepare::<PolkadotSubCommands, PolkadotSubParams, _>(
		&version,
		"parity-polkadot",
		std::env::args(),
	) {
		cli::ParseAndPrepare::Run(cmd) => cmd.run(load_spec, worker,
		|worker, _cli_args, custom_args, mut config| {
			info!("{}", version.name);
			info!("  version {}", config.full_version());
			info!("  by {}, 2017-2019", version.author);
			info!("Chain specification: {}", config.chain_spec.name());
			if config.chain_spec.name().starts_with("Kusama") {
				info!("----------------------------");
				info!("This chain is not in any way");
				info!("      endorsed by the       ");
				info!("     KUSAMA FOUNDATION      ");
				info!("----------------------------");
			}
			info!("Node name: {}", config.name);
			info!("Roles: {}", display_role(&config));
			config.custom = worker.configuration();
			config.custom.authority_discovery_enabled = custom_args.authority_discovery_enabled;
			let runtime = Runtime::new().map_err(|e| format!("{:?}", e))?;
			match config.roles {
				service::Roles::LIGHT =>
					run_until_exit(
						runtime,
						service::new_light(config).map_err(|e| format!("{:?}", e))?,
						worker
					),
				_ => run_until_exit(
						runtime,
						service::new_full(config).map_err(|e| format!("{:?}", e))?,
						worker
					),
			}.map_err(|e| format!("{:?}", e))
		}),
		cli::ParseAndPrepare::BuildSpec(cmd) => cmd.run::<NoCustom, _, _, _>(load_spec),
		cli::ParseAndPrepare::ExportBlocks(cmd) => cmd.run_with_builder::<(), _, _, _, _, _, _>(|config|
			Ok(service::new_chain_ops(config)?), load_spec, worker),
		cli::ParseAndPrepare::ImportBlocks(cmd) => cmd.run_with_builder::<(), _, _, _, _, _, _>(|config|
			Ok(service::new_chain_ops(config)?), load_spec, worker),
		cli::ParseAndPrepare::CheckBlock(cmd) => cmd.run_with_builder::<(), _, _, _, _, _, _>(|config|
			Ok(service::new_chain_ops(config)?), load_spec, worker),
		cli::ParseAndPrepare::PurgeChain(cmd) => cmd.run(load_spec),
		cli::ParseAndPrepare::RevertChain(cmd) => cmd.run_with_builder::<(), _, _, _, _, _>(|config|
			Ok(service::new_chain_ops(config)?), load_spec),
		cli::ParseAndPrepare::CustomCommand(PolkadotSubCommands::ValidationWorker(args)) => {
			if cfg!(feature = "browser") {
				Err(error::Error::Input("Cannot run validation worker in browser".into()))
			} else {
				#[cfg(not(feature = "browser"))]
				service::run_validation_worker(&args.mem_id)?;
				Ok(())
			}
		}
	}
}

fn run_until_exit<T, SC, B, CE, W>(
	mut runtime: Runtime,
	service: T,
	worker: W,
) -> error::Result<()>
	where
		T: AbstractService<Block = service::Block, RuntimeApi = service::RuntimeApi,
			SelectChain = SC, Backend = B, NetworkSpecialization = service::PolkadotProtocol, CallExecutor = CE>,
		SC: service::SelectChain<service::Block> + 'static,
		B: service::Backend<service::Block, service::Blake2Hasher> + 'static,
		CE: service::CallExecutor<service::Block, service::Blake2Hasher> + Clone + Send + Sync + 'static,
		W: Worker,
{
	let (exit_send, exit) = oneshot::channel();

	let executor = runtime.executor();
	let informant = cli::informant::build(&service);
	let future = select(exit, informant)
		.map(|_| Ok(()))
		.compat();

	executor.spawn(future);

	// we eagerly drop the service so that the internal exit future is fired,
	// but we need to keep holding a reference to the global telemetry guard
	let _telemetry = service.telemetry();

	let work = worker.work(&service, WrappedExecutor(executor));
	let service = service
		.map_err(|err| error!("Error while running Service: {}", err))
		.compat();
	let future = select(service, work)
		.map(|_| Ok::<_, ()>(()))
		.compat();
	let _ = runtime.block_on(future);
	let _ = exit_send.send(());

	use futures01::Future;

	// TODO [andre]: timeout this future substrate/#1318
	let _ = runtime.shutdown_on_idle().wait();

	Ok(())
}
