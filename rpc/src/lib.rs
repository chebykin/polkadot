// Copyright 2019 Parity Technologies (UK) Ltd.
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

//! Polkadot-specific RPCs implementation.

#![warn(missing_docs)]

use std::sync::Arc;

use polkadot_primitives::{Block, AccountId, Nonce, Balance};
use sp_runtime::traits::ProvideRuntimeApi;
use txpool_api::TransactionPool;
use polkadot_runtime::UncheckedExtrinsic;

/// A type representing all RPC extensions.
pub type RpcExtension = jsonrpc_core::IoHandler<sc_rpc::Metadata>;

/// Instantiate all RPC extensions.
pub fn create_full<C, P>(client: Arc<C>, pool: Arc<P>) -> RpcExtension where
	C: ProvideRuntimeApi,
	C: client::blockchain::HeaderBackend<Block>,
	C: Send + Sync + 'static,
	C::Api: frame_rpc_system::AccountNonceApi<Block, AccountId, Nonce>,
	C::Api: pallet_transaction_payment_rpc::TransactionPaymentRuntimeApi<Block, Balance, UncheckedExtrinsic>,
	P: TransactionPool + Sync + Send + 'static,
{
	use frame_rpc_system::{FullSystem, SystemApi};
	use pallet_transaction_payment_rpc::{TransactionPayment, TransactionPaymentApi};

	let mut io = jsonrpc_core::IoHandler::default();
	io.extend_with(
		SystemApi::to_delegate(FullSystem::new(client.clone(), pool))
	);
	io.extend_with(
		TransactionPaymentApi::to_delegate(TransactionPayment::new(client))
	);
	io
}

/// Instantiate all RPC extensions for light node.
pub fn create_light<C, P, F>(
	client: Arc<C>,
	remote_blockchain: Arc<dyn client::light::blockchain::RemoteBlockchain<Block>>,
	fetcher: Arc<F>,
	pool: Arc<P>,
) -> RpcExtension
	where
		C: ProvideRuntimeApi,
		C: client::blockchain::HeaderBackend<Block>,
		C: Send + Sync + 'static,
		C::Api: frame_rpc_system::AccountNonceApi<Block, AccountId, Nonce>,
		C::Api: pallet_transaction_payment_rpc::TransactionPaymentRuntimeApi<Block, Balance, UncheckedExtrinsic>,
		P: TransactionPool + Sync + Send + 'static,
		F: client::light::fetcher::Fetcher<Block> + 'static,
{
	use frame_rpc_system::{LightSystem, SystemApi};

	let mut io = jsonrpc_core::IoHandler::default();
	io.extend_with(
		SystemApi::<AccountId, Nonce>::to_delegate(LightSystem::new(client, remote_blockchain, fetcher, pool))
	);
	io
}
