//! `routes/ws.rs` — WebSocket endpoints.
//!
//! Laravel has no counterpart, because Laravel does not hold sockets: it
//! publishes to a relay and something else holds them. Rainier can do both —
//! see [`channels`](crate::routes::channels) for the other one.
//!
//! These run on the **same port** as the HTTP routes. A socket connection
//! starts as a `GET` asking to upgrade, so there is nothing to start
//! separately and nothing to keep in step.

use std::sync::Arc;

use rainier_framework::prelude::*;
use rainier_framework::websocket::Message;

/// Declare this application's socket routes.
pub fn routes(rooms: Arc<Rooms>) -> WebSocketRoutes {
    WebSocketRoutes::new().add("/ws/rooms/{room}", Chat { rooms })
}

/// A room whose members hear each other — the smallest thing a socket is for.
///
/// Note what it does *not* do: keep its own list of who is where. `Rooms` is
/// that list, `leave_all` is one call, and a handler that tracked it as well
/// would be keeping a second copy that drifts.
pub struct Chat {
    rooms: Arc<Rooms>,
}

#[async_trait]
impl WebSocketHandler for Chat {
    /// Only signed-in callers.
    ///
    /// Runs **before** the handshake, with the HTTP request — so the token the
    /// `auth` middleware would have read is right here. Returning `false`
    /// answers `403` and no socket is created.
    ///
    /// A socket is a route. It needs the same thought about who may reach it,
    /// and the default — everyone — is right for a public feed and wrong for
    /// this one.
    fn authorize(&self, request: &Request) -> bool {
        request.header("authorization").is_some_and(|value| value.starts_with("Bearer "))
    }

    async fn on_connect(&self, socket: &Socket) -> Result<()> {
        let room = self.room(socket);

        self.rooms.join(&room, socket.clone());
        self.rooms.send_except(&room, socket.id(), Message::text("someone joined"));

        socket.send_json(&serde_json::json!({
            "joined": room,
            "members": self.rooms.count(&room),
        }))
    }

    async fn on_message(&self, socket: &Socket, message: Message) -> Result<()> {
        let Some(text) = message.as_text() else {
            // A chat is text. Saying so beats relaying bytes nobody renders.
            return socket.send(Message::text("this room speaks text"));
        };

        self.rooms.send_except(&self.room(socket), socket.id(), Message::text(text));
        Ok(())
    }

    /// Runs however the connection ended — a clean close, a dropped
    /// connection, a closed laptop. Without it the room keeps a handle to a
    /// socket nobody is on the other end of.
    async fn on_close(&self, socket: &Socket) {
        let room = self.room(socket);

        self.rooms.leave_all(socket.id());
        self.rooms.send(&room, Message::text("someone left"));
    }
}

impl Chat {
    /// The room from the path — `/ws/rooms/{room}`.
    fn room(&self, socket: &Socket) -> String {
        socket.param("room").unwrap_or("lobby").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rainier_framework::http::Method;
    use rainier_framework::websocket::{Outbound, SocketId};
    use tokio::sync::mpsc;

    fn socket(room: &str) -> (Socket, mpsc::UnboundedReceiver<Outbound>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let socket = Socket::new(
            SocketId::next(),
            format!("/ws/rooms/{room}"),
            vec![("room".into(), room.to_string())],
            tx,
        );
        (socket, rx)
    }

    fn chat() -> Chat {
        Chat { rooms: Arc::new(Rooms::new()) }
    }

    fn text_of(outbound: Outbound) -> String {
        match outbound {
            Outbound::Send(Message::Text(text)) => text,
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn the_route_is_declared() {
        let routes = routes(Arc::new(Rooms::new()));

        assert_eq!(routes.patterns(), vec!["/ws/rooms/{room}"]);
        assert!(routes.match_path("/ws/rooms/lobby").is_some());
        assert!(routes.match_path("/ws/rooms").is_none());
    }

    #[test]
    fn a_caller_with_no_token_is_refused_before_the_handshake() {
        let chat = chat();

        assert!(!chat.authorize(&Request::builder().method(Method::GET).build()));
        assert!(chat.authorize(
            &Request::builder().method(Method::GET).header("authorization", "Bearer x").build()
        ));
    }

    #[tokio::test]
    async fn joining_tells_the_room_and_greets_the_newcomer() {
        // A handler is testable by calling it: `Socket` is a channel, so none
        // of this needs a network.
        let chat = chat();
        let (first, mut first_rx) = socket("lobby");
        let (second, mut second_rx) = socket("lobby");

        chat.on_connect(&first).await.unwrap();
        let _ = first_rx.try_recv();

        chat.on_connect(&second).await.unwrap();

        assert_eq!(
            text_of(first_rx.try_recv().expect("the room hears about it")),
            "someone joined"
        );

        let greeting = text_of(second_rx.try_recv().expect("the newcomer is greeted"));
        let payload: serde_json::Value = serde_json::from_str(&greeting).unwrap();
        assert_eq!(payload["joined"], "lobby");
        assert_eq!(payload["members"], 2);
    }

    #[tokio::test]
    async fn a_message_reaches_everyone_but_its_sender() {
        let chat = chat();
        let (first, mut first_rx) = socket("lobby");
        let (second, mut second_rx) = socket("lobby");

        chat.on_connect(&first).await.unwrap();
        chat.on_connect(&second).await.unwrap();
        while first_rx.try_recv().is_ok() {}
        while second_rx.try_recv().is_ok() {}

        chat.on_message(&first, Message::text("hello")).await.unwrap();

        assert!(first_rx.try_recv().is_err(), "the sender has already shown their own message");
        assert_eq!(text_of(second_rx.try_recv().expect("the other one hears it")), "hello");
    }

    #[tokio::test]
    async fn two_rooms_do_not_hear_each_other() {
        let chat = chat();
        let (lobby, mut lobby_rx) = socket("lobby");
        let (kitchen, mut kitchen_rx) = socket("kitchen");

        chat.on_connect(&lobby).await.unwrap();
        chat.on_connect(&kitchen).await.unwrap();
        while lobby_rx.try_recv().is_ok() {}
        while kitchen_rx.try_recv().is_ok() {}

        chat.on_message(&lobby, Message::text("only for the lobby")).await.unwrap();

        assert!(kitchen_rx.try_recv().is_err(), "the other room should hear nothing");
    }

    #[tokio::test]
    async fn a_binary_frame_gets_an_explanation_rather_than_silence() {
        let chat = chat();
        let (socket, mut rx) = socket("lobby");

        chat.on_message(&socket, Message::binary(vec![1, 2, 3])).await.unwrap();

        assert_eq!(text_of(rx.try_recv().expect("something comes back")), "this room speaks text");
    }

    #[tokio::test]
    async fn closing_leaves_the_room_empty() {
        let chat = chat();
        let (socket, _rx) = socket("lobby");

        chat.on_connect(&socket).await.unwrap();
        assert_eq!(chat.rooms.count("lobby"), 1);

        chat.on_close(&socket).await;
        assert_eq!(chat.rooms.count("lobby"), 0);
    }
}
