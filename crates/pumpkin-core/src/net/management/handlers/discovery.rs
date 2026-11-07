//! RPC methods of the discovery group.

use super::super::rpc::RpcError;
use super::{OPENRPC_SCHEMA, check_no_params};
use serde_json::Value;

/// Handles the arm `"rpc.discover"`.
pub(super) fn discover(params: Option<&Value>) -> Result<Value, RpcError> {
    check_no_params(params)?;
    Ok(OPENRPC_SCHEMA.clone())
}
