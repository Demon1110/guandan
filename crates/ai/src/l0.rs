//! AI-L0：最小策略。
//!
//! 规则只此三条，且全部只依赖 `view(seat)`：
//! 1. **他座 `lastPlay`**（真跟牌）→ `Pass`。L0 不做牌力比较，跟牌一律放弃；
//! 2. **自己 `lastPlay`**（回合被全员过牌清空后出牌权交回本座）→ 领出；
//! 3. **无 `lastPlay`**（领出）→ 出手中牌 id 最小的一张单张（单张在领出态恒合法）。
//!
//! 判据只比 `lastPlay.seat` 与自己的座位号，不看 `lastPlay` 是否存在——因为引擎把出牌权
//! 交回最后出牌人时仍保留 `last_play` 供视图展示，此时 `lastPlay.seat == 自己` 其实是领出。

use gf_engine::contract::Action;
use gf_table_runtime::BotPolicy;

/// AI-L0 最小策略 bot（无状态、确定性）。
#[derive(Clone, Copy, Debug, Default)]
pub struct L0Bot;

impl BotPolicy for L0Bot {
    fn decide(&self, view_bytes: &[u8], seat: u8) -> Action {
        if crate::view_scan::is_following(view_bytes, seat) {
            return Action::Pass;
        }
        match crate::view_scan::hand_cards(view_bytes).first() {
            Some(&card) => Action::Play { cards: vec![card] },
            None => Action::Pass,
        }
    }
}
