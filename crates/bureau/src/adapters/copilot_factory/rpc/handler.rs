use std::io;

use serde_json::Value;

use super::RpcFault;

/// A new outbound request emitted by a notification handler.
///
/// The dispatcher assigns its ID and observes its reply without a waiting caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackRequest {
    pub method: String,
    /// Null omits the wire member, as with [`super::Client::call_without_params`].
    pub params: Value,
}

/// Synchronous callbacks owned by the dispatcher, never called concurrently.
///
/// Implementations may perform bounded persistence, but must not wait for another
/// RPC on this connection. Authorization belongs to the supplied implementation;
/// the default request handler grants nothing.
pub trait Handler: Send + 'static {
    /// Handles an explicit reverse request.
    ///
    /// # Errors
    ///
    /// Returns the JSON-RPC fault to send to the peer, without closing the client.
    fn request(&mut self, _method: &str, _params: &Value) -> Result<Value, RpcFault> {
        Err(RpcFault {
            code: -32601,
            message: "Method not found".to_owned(),
            data: None,
        })
    }

    /// Observes a notification and returns new outbound requests to send immediately.
    ///
    /// Returned requests use the same lease fence and response observer as ordinary
    /// calls. No callback may wait for their replies or retain an owning client.
    ///
    /// # Errors
    ///
    /// A persistence or validation failure closes the connection.
    fn notification(&mut self, _method: &str, _params: &Value) -> io::Result<Vec<CallbackRequest>> {
        Ok(Vec::new())
    }

    /// Observes a reply before completing its caller, even if that caller left.
    /// Also observes acknowledgements of notification-generated requests.
    ///
    /// # Errors
    ///
    /// A persistence or validation failure closes the connection.
    fn response(
        &mut self,
        _method: &str,
        _params: &Value,
        _result: &Result<Value, RpcFault>,
    ) -> io::Result<()> {
        Ok(())
    }

    /// Rechecks a lease or other precondition immediately before an outbound write.
    ///
    /// # Errors
    ///
    /// Failure prevents the write and closes the connection.
    fn before_request(&mut self, _method: &str, _params: &Value) -> io::Result<()> {
        Ok(())
    }
}

impl Handler for () {}
