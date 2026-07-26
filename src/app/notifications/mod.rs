//! Notifications — `app/Notifications`.
//!
//! # A notification is not an event
//!
//! They are easy to confuse, because both are "something happened, tell the
//! interested parties". The difference is who decides, and when:
//!
//! | | [Event](crate::app::models::PostPublished) | Notification |
//! |---|---|---|
//! | What it is | a **fact**: this happened | a **message**: someone should be told |
//! | Who receives it | whoever subscribed, at boot | one named recipient, per send |
//! | Who decides | the listener list, fixed for the process | `via()`, per recipient |
//! | Where it goes | in-process function calls | out of the process — email, SMS, a row |
//! | If nobody is listening | nothing happens, and that is fine | nothing was delivered, which is a bug |
//!
//! A rule of thumb: if you can describe it without naming a person, it is an
//! event. `PostPublished` is a fact about a post — the search index and the
//! cache care about it too, and they are not "recipients".
//! [`PostLive`](post_live::PostLive) is a message to the author, and asking who
//! it is for is the whole point.
//!
//! # This application uses both, in one chain
//!
//! ```text
//! controller  →  Event::dispatch(PostPublished)      the fact
//!                       ↓
//! listener    →  Queue::dispatch(NotifyAuthor)       one subscriber's reaction
//!                       ↓
//! job         →  Notify::send(&author, &PostLive)    a message, to a person
//!                       ↓
//! channels    →  mail + database                     chosen by `via()`
//! ```
//!
//! Each arrow is a place the next step could change without the previous one
//! knowing. The controller does not know an email goes out; the event does not
//! know a queue exists; the notification does not know the address.

pub mod post_live;

pub use post_live::PostLive;
