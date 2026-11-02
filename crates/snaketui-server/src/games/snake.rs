//! Snake as this server plays it: quick play has two free-for-all pools on
//! the quick-play board, classic and with guns, each rated apart by
//! Weng-Lin.

use std::sync::Arc;
use std::time::Duration;

use game_api::{Entrant, GameId, MIN_PLAYERS};
use shell_tui::GameUi;
use snake_core::config::Rules;
use snake_core::round::{
    GUNS_POOL, MAX_PLAYERS, QUICK_POOL, RoomKind, RoomSettings, Snake, bot_entrant,
    quick_guns_rules, quick_rules,
};
use snake_tui::SnakeUi;

use super::{
    AnyRoomView, GameInfo, GameServer, PoolInfo, PoolKey, RatingSystem, decode, decode_view,
};
use crate::rating::{SNAKE_GUNS, SNAKE_QUICK};
use crate::room::NewRoom;

/// Bots in a match against bots.
const BOT_MATCH_BOTS: usize = 3;

static INFO: GameInfo = GameInfo {
    id: GameId::Snake,
    title: "Snake",
    ssh_users: &["snake"],
    rating: RatingSystem::WengLin,
    categories: &[SNAKE_QUICK.category, SNAKE_GUNS.category],
    pools: &[
        PoolInfo {
            id: QUICK_POOL,
            category: SNAKE_QUICK.category,
            seats: MIN_PLAYERS..=MAX_PLAYERS,
            fill_wait: Duration::from_secs(10),
        },
        PoolInfo {
            id: GUNS_POOL,
            category: SNAKE_GUNS.category,
            seats: MIN_PLAYERS..=MAX_PLAYERS,
            fill_wait: Duration::from_secs(10),
        },
    ],
};

/// Quick play's classic pool.
pub fn quick_pool() -> PoolKey {
    PoolKey {
        game: GameId::Snake,
        pool: QUICK_POOL.to_string(),
    }
}

/// A matched room on the quick-play board, with guns for that pool.
fn match_settings(pool: &PoolInfo) -> RoomSettings {
    let rules = if pool.id == GUNS_POOL {
        quick_guns_rules()
    } else {
        quick_rules()
    };
    RoomSettings::new(rules, RoomKind::Match).expect("the quick-play board fits a full room")
}

pub struct SnakeServer;

impl GameServer for SnakeServer {
    fn info(&self) -> &'static GameInfo {
        &INFO
    }

    fn decode_view(&self, body: &[u8]) -> Option<Arc<dyn AnyRoomView>> {
        decode_view::<Snake>(body)
    }

    fn new_match(&self, pool: &PoolInfo, seed: u64, players: Vec<Entrant>) -> Box<dyn NewRoom> {
        Box::new(game_api::Room::<Snake>::new_match(
            match_settings(pool),
            seed,
            players,
        ))
    }

    /// The pool's rules against a few bots; Snake's bots are all alike.
    fn bot_match(
        &self,
        pool: &PoolInfo,
        seed: u64,
        who: Entrant,
        _: &[u8],
    ) -> Option<Box<dyn NewRoom>> {
        let mut players = vec![who];
        players.extend((0..BOT_MATCH_BOTS).map(bot_entrant));
        Some(Box::new(game_api::Room::<Snake>::new_match(
            match_settings(pool),
            seed,
            players,
        )))
    }

    fn private_room(&self, settings: &[u8], seed: u64) -> Result<Box<dyn NewRoom>, &'static str> {
        let settings = decode::<Rules>(settings)
            .and_then(|rules| RoomSettings::new(rules, RoomKind::Private))
            .ok_or("Your board is too small for two snakes. Make it bigger in Single player.")?;
        Ok(Box::new(game_api::Room::<Snake>::new(settings, seed)))
    }

    fn ui(&self) -> Box<dyn GameUi> {
        Box::new(SnakeUi::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_pool_plays_on_its_own_board() {
        let [classic, guns] = &INFO.pools else {
            panic!("two pools");
        };
        assert!(!match_settings(classic).game.guns);
        assert!(match_settings(guns).game.guns);
        assert_eq!(guns.category, SNAKE_GUNS.category, "rated apart");
        assert_eq!(super::super::rating_key("snake", "guns"), Some(SNAKE_GUNS));
    }
}
