//! Where items come from besides recipes: the Horizon JEI kit's source pages (World Gen, drops, loot, trades),
//! made on the server from its data packs and sent to the client (`server`), shown as JEI cards (`client`).

pub(crate) mod client;
pub(crate) mod index;
mod math;
mod nbt;
pub(crate) mod server;
