//! Pidor duel: create, accept, move, win check, stats for achievements.
//!
//! Infinite tic-tac-toe (no draw): 3-piece limit — each player has at most 3 pieces;
//! when placing the 4th, the oldest piece is removed so the game continues until 3 in a row.
//! Clicks from the non-current player are ignored (handler returns before make_move).

use sqlx::PgPool;

use crate::db::models::DuelGame;
use crate::error::AppError;

const ACCEPT_TIMEOUT_SECS: i64 = 60;

const DUEL_GAME_SELECT: &str = "id, chat_id, invite_message_id, message_id, challenger_tg_id, \
    invited_tg_id, player1_tg_id, player2_tg_id, board, cell_filled_at, turn, status, \
    winner_tg_id, created_at, last_move_at, game_type, game_state";
pub const ACTIVE_INACTIVITY_TIMEOUT_SECS: i64 = 60;
const MAX_PIECES_PER_PLAYER: usize = 3;

const LINES: [[usize; 3]; 8] = [
    [0, 1, 2],
    [3, 4, 5],
    [6, 7, 8],
    [0, 3, 6],
    [1, 4, 7],
    [2, 5, 8],
    [0, 4, 8],
    [2, 4, 6],
];

fn board_winner(board: &str) -> Option<char> {
    let b = board.as_bytes();
    if b.len() < 9 {
        return None;
    }
    for line in &LINES {
        let c = b[line[0]] as char;
        if c != ' ' && c == b[line[1]] as char && c == b[line[2]] as char {
            return Some(c);
        }
    }
    None
}

/// 3-piece limit: if player already has 3 pieces, remove the oldest. Returns (board, cell_filled_at).
fn apply_three_piece_limit(board: &str, cell_filled_at: &str, mark: char) -> (String, String) {
    let timestamps: Vec<i64> = cell_filled_at
        .split(',')
        .map(|s| s.trim().parse().unwrap_or(0))
        .chain(std::iter::repeat(0))
        .take(9)
        .collect();
    let mut chars: Vec<char> = board.chars().chain(std::iter::repeat(' ')).take(9).collect();
    let indices_with_mark: Vec<usize> = (0..9).filter(|&i| chars[i] == mark).collect();
    if indices_with_mark.len() < MAX_PIECES_PER_PLAYER {
        return (chars.into_iter().collect(), cell_filled_at.to_string());
    }
    let oldest_idx = indices_with_mark
        .into_iter()
        .min_by_key(|&i| timestamps.get(i).copied().unwrap_or(0))
        .unwrap();
    let mut new_ts = timestamps.clone();
    chars[oldest_idx] = ' ';
    new_ts[oldest_idx] = 0;
    let cell_filled_at_str = new_ts
        .iter()
        .map(|t| t.to_string())
        .collect::<Vec<_>>()
        .join(",");
    (chars.into_iter().collect(), cell_filled_at_str)
}

pub fn check_win(board: &str) -> Option<char> {
    board_winner(board)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_winner_detects_row_win() {
        let board = "111      ";
        assert_eq!(board_winner(board), Some('1'));
    }

    #[test]
    fn board_winner_detects_column_win() {
        let board = "1  1  1  ";
        assert_eq!(board_winner(board), Some('1'));
    }

    #[test]
    fn board_winner_detects_diagonal_win() {
        let board = "1   1   1";
        assert_eq!(board_winner(board), Some('1'));
    }

    #[test]
    fn board_winner_detects_anti_diagonal_win() {
        let board = "  2 2 2  ";
        assert_eq!(board_winner(board), Some('2'));
    }

    #[test]
    fn board_winner_none_when_no_win() {
        let board = "12 21 12 ";
        assert_eq!(board_winner(board), None);
    }

    #[test]
    fn apply_three_piece_limit_removes_oldest_piece() {
        let board = "1 1 1    ";
        let ts = "1,0,3,0,5,0,0,0,0";
        let (new_board, new_ts) = apply_three_piece_limit(board, ts, '1');
        assert_eq!(new_board.chars().nth(0), Some(' '));
        assert_eq!(new_ts.split(',').next().unwrap(), "0");
        assert_eq!(new_board.chars().nth(2), Some('1'));
        assert_eq!(new_board.chars().nth(4), Some('1'));
    }

    #[test]
    fn check_win_delegates_to_board_winner() {
        let board = "   222   ";
        assert_eq!(check_win(board), Some('2'));
    }

    #[test]
    fn elo_expected_equal_players() {
        let e = elo_expected(1000, 1000);
        assert!((e - 0.5).abs() < 0.01);
    }

    #[test]
    fn elo_expected_stronger_player() {
        let e = elo_expected(1400, 1000);
        assert!(e > 0.85);
    }

    #[test]
    fn elo_rank_tiers() {
        // These compare against LOCALE strings, which are loaded at runtime.
        // Just verify the function returns a non-empty string for each range.
        assert!(!elo_rank(0).is_empty());
        assert!(!elo_rank(500).is_empty());
        assert!(!elo_rank(800).is_empty());
        assert!(!elo_rank(1200).is_empty());
        assert!(!elo_rank(1600).is_empty());
        assert!(!elo_rank(2500).is_empty());
        assert!(!elo_rank(4000).is_empty());
        assert!(!elo_rank(5500).is_empty());
        assert!(!elo_rank(7000).is_empty());
        assert!(!elo_rank(9000).is_empty());
        assert!(!elo_rank(10000).is_empty());
    }
}

/// Create pending duel. invite_message_id can be set after sending the message.
pub async fn create(
    pool: &PgPool,
    chat_id: i64,
    challenger_tg_id: i64,
    invited_tg_id: Option<i64>,
    invite_message_id: Option<i64>,
) -> Result<DuelGame, AppError> {
    let row = sqlx::query_as::<_, DuelGame>(
        &format!("INSERT INTO duel_game (chat_id, invite_message_id, challenger_tg_id, invited_tg_id, status)
        VALUES ($1, $2, $3, $4, 'pending_accept')
        RETURNING {DUEL_GAME_SELECT}"),
    )
    .bind(chat_id)
    .bind(invite_message_id)
    .bind(challenger_tg_id)
    .bind(invited_tg_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn set_invite_message_id(pool: &PgPool, id: i32, message_id: i64) -> Result<(), AppError> {
    sqlx::query("UPDATE duel_game SET invite_message_id = $1 WHERE id = $2")
        .bind(message_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_message_id(pool: &PgPool, id: i32, message_id: i64) -> Result<(), AppError> {
    sqlx::query("UPDATE duel_game SET message_id = $1 WHERE id = $2")
        .bind(message_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_by_id(pool: &PgPool, id: i32) -> Result<Option<DuelGame>, AppError> {
    let row = sqlx::query_as::<_, DuelGame>(
        &format!("SELECT {DUEL_GAME_SELECT} FROM duel_game WHERE id = $1"),
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_pending_or_active_by_chat(pool: &PgPool, chat_id: i64) -> Result<Option<DuelGame>, AppError> {
    let row = sqlx::query_as::<_, DuelGame>(
        &format!("SELECT {DUEL_GAME_SELECT} FROM duel_game
        WHERE chat_id = $1 AND status IN ('pending_accept', 'active')
        ORDER BY id DESC LIMIT 1"),
    )
    .bind(chat_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn accept(
    pool: &PgPool,
    id: i32,
    accepter_tg_id: i64,
) -> Result<Option<DuelGame>, AppError> {
    let d = match get_by_id(pool, id).await? {
        Some(x) => x,
        None => return Ok(None),
    };
    if d.status != "pending_accept" {
        return Ok(None);
    }
    if d.challenger_tg_id == accepter_tg_id {
        return Ok(None);
    }
    if let Some(inv) = d.invited_tg_id {
        if inv != accepter_tg_id {
            return Ok(None);
        }
    }
    let created_secs = d.created_at.timestamp();
    let now_secs = chrono::Utc::now().timestamp();
    if now_secs - created_secs >= ACCEPT_TIMEOUT_SECS {
        let _ = set_cancelled(pool, id).await;
        return Ok(None);
    }

    // Scope rng so ThreadRng is dropped before the .execute().await below.
    let (player1_tg_id, player2_tg_id, game_type) = {
        use rand::RngExt;
        let mut rng = rand::rng();
        let (p1, p2) = if rng.random::<bool>() {
            (d.challenger_tg_id, accepter_tg_id)
        } else {
            (accepter_tg_id, d.challenger_tg_id)
        };
        let game_types = ["tictactoe", "dice", "coin", "rps"];
        let gt = game_types[rng.random_range(0..game_types.len())];
        (p1, p2, gt)
    };

    sqlx::query(
        r#"
        UPDATE duel_game
        SET status = 'active', player1_tg_id = $1, player2_tg_id = $2, turn = 1,
            last_move_at = NOW(), game_type = $4
        WHERE id = $3
        "#,
    )
    .bind(player1_tg_id)
    .bind(player2_tg_id)
    .bind(id)
    .bind(game_type)
    .execute(pool)
    .await?;

    get_by_id(pool, id).await
}

pub async fn decline(pool: &PgPool, id: i32) -> Result<(), AppError> {
    sqlx::query("UPDATE duel_game SET status = 'declined' WHERE id = $1 AND status = 'pending_accept'")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_cancelled(pool: &PgPool, id: i32) -> Result<(), AppError> {
    sqlx::query("UPDATE duel_game SET status = 'cancelled' WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Active duels with no move for at least inactivity_secs. Returns (id, chat_id, message_id).
pub async fn get_stale_active_duels(
    pool: &PgPool,
    inactivity_secs: i64,
) -> Result<Vec<(i32, i64, Option<i64>)>, AppError> {
    let rows = sqlx::query_as::<_, (i32, i64, Option<i64>)>(
        r#"
        SELECT id, chat_id, message_id FROM duel_game
        WHERE status = 'active' AND last_move_at IS NOT NULL
          AND last_move_at < NOW() - ($1::bigint * INTERVAL '1 second')
        "#,
    )
    .bind(inactivity_secs)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn make_move(
    pool: &PgPool,
    id: i32,
    cell: usize,
    player_tg_id: i64,
) -> Result<(DuelGame, Option<i64>), AppError> {
    if cell >= 9 {
        return Err(AppError::GameLogic("invalid cell".into()));
    }
    let mut tx = pool.begin().await?;
    let mut d = match sqlx::query_as::<_, DuelGame>(
        &format!("SELECT {DUEL_GAME_SELECT} FROM duel_game WHERE id = $1 FOR UPDATE"),
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    {
        Some(x) => x,
        None => {
            let _ = tx.rollback().await;
            return Err(AppError::NotFound("duel not found".into()));
        }
    };
    if d.status != "active" {
        let _ = tx.rollback().await;
        return Err(AppError::GameLogic("duel not active".into()));
    }
    let current_player = if d.turn == 1 {
        d.player1_tg_id
    } else {
        d.player2_tg_id
    };
    if current_player != Some(player_tg_id) {
        let _ = tx.rollback().await;
        return Err(AppError::GameLogic("not your turn".into()));
    }

    let mark = if d.turn == 1 { '1' } else { '2' };
    let (board, cell_filled_at) = apply_three_piece_limit(&d.board, &d.cell_filled_at, mark);
    d.board = board.clone();
    d.cell_filled_at = cell_filled_at.clone();

    let mut chars: Vec<char> = d.board.chars().collect();
    if chars.len() < 9 {
        chars.resize(9, ' ');
    }
    if chars[cell] != ' ' {
        return Err(AppError::GameLogic("cell occupied".into()));
    }
    let now_secs = chrono::Utc::now().timestamp();
    chars[cell] = mark;
    let new_board: String = chars.into_iter().collect();

    let mut timestamps: Vec<i64> = d
        .cell_filled_at
        .split(',')
        .map(|s| s.trim().parse().unwrap_or(0))
        .chain(std::iter::repeat(0))
        .take(9)
        .collect();
    timestamps[cell] = now_secs;
    let new_cell_filled_at = timestamps
        .iter()
        .map(|t| t.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let next_turn = if d.turn == 1 { 2 } else { 1 };

    sqlx::query(
        r#"
        UPDATE duel_game SET board = $1, cell_filled_at = $2, turn = $3, last_move_at = NOW() WHERE id = $4
        "#,
    )
    .bind(&new_board)
    .bind(&new_cell_filled_at)
    .bind(next_turn)
    .bind(id)
    .execute(&mut *tx)
    .await?;

    let winner = check_win(&new_board).map(|c| {
        if c == '1' {
            d.player1_tg_id.unwrap()
        } else {
            d.player2_tg_id.unwrap()
        }
    });

    if let Some(winner_tg_id) = winner {
        sqlx::query(
            "UPDATE duel_game SET status = 'finished', winner_tg_id = $1 WHERE id = $2",
        )
        .bind(winner_tg_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    d.board = new_board;
    d.cell_filled_at = new_cell_filled_at;
    d.turn = next_turn;
    if let Some(w) = winner {
        d.winner_tg_id = Some(w);
        d.status = "finished".to_string();
    }
    Ok((d, winner))
}

pub async fn count_wins(pool: &PgPool, tg_id: i64) -> Result<i64, AppError> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::bigint FROM duel_game WHERE status = 'finished' AND winner_tg_id = $1",
    )
    .bind(tg_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn count_losses(pool: &PgPool, tg_id: i64) -> Result<i64, AppError> {
    let row: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)::bigint FROM duel_game
        WHERE status = 'finished' AND winner_tg_id IS NOT NULL
          AND (player1_tg_id = $1 OR player2_tg_id = $1) AND winner_tg_id != $1
        "#,
    )
    .bind(tg_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn count_played(pool: &PgPool, tg_id: i64) -> Result<i64, AppError> {
    let row: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)::bigint FROM duel_game
        WHERE status = 'finished' AND (player1_tg_id = $1 OR player2_tg_id = $1)
        "#,
    )
    .bind(tg_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

// ── Elo rating ──────────────────────────────────────────

use crate::db::models::DuelElo;

const ELO_K: f64 = 32.0;

const ELO_SELECT: &str =
    "id, chat_id, tg_id, elo, wins, losses, pidor_elo, huya_elo";

pub async fn get_or_create_elo(pool: &PgPool, chat_id: i64, tg_id: i64) -> Result<DuelElo, AppError> {
    let row = sqlx::query_as::<_, DuelElo>(
        &format!("INSERT INTO duel_elo (chat_id, tg_id) VALUES ($1, $2)
         ON CONFLICT (chat_id, tg_id) DO UPDATE SET chat_id = EXCLUDED.chat_id
         RETURNING {ELO_SELECT}"),
    )
    .bind(chat_id)
    .bind(tg_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

fn elo_expected(a: i32, b: i32) -> f64 {
    1.0 / (1.0 + 10f64.powf((b - a) as f64 / 400.0))
}

pub async fn update_elo_after_duel(
    pool: &PgPool,
    chat_id: i64,
    winner_tg_id: i64,
    loser_tg_id: i64,
) -> Result<(DuelElo, DuelElo), AppError> {
    let w = get_or_create_elo(pool, chat_id, winner_tg_id).await?;
    let l = get_or_create_elo(pool, chat_id, loser_tg_id).await?;

    let exp_w = elo_expected(w.elo, l.elo);
    let exp_l = 1.0 - exp_w;
    let new_w_elo = (w.elo as f64 + ELO_K * (1.0 - exp_w)).round() as i32;
    let new_l_elo = (l.elo as f64 + ELO_K * (0.0 - exp_l)).round().max(0.0) as i32;

    sqlx::query("UPDATE duel_elo SET elo = $1, wins = wins + 1 WHERE id = $2")
        .bind(new_w_elo)
        .bind(w.id)
        .execute(pool)
        .await?;
    sqlx::query("UPDATE duel_elo SET elo = $1, losses = losses + 1 WHERE id = $2")
        .bind(new_l_elo)
        .bind(l.id)
        .execute(pool)
        .await?;

    let w_updated = DuelElo { elo: new_w_elo, wins: w.wins + 1, ..w };
    let l_updated = DuelElo { elo: new_l_elo, losses: l.losses + 1, ..l };
    Ok((w_updated, l_updated))
}

/// Add points to pidor_elo (awarded for pidor wins and bet wins).
pub async fn add_pidor_elo(pool: &PgPool, chat_id: i64, tg_id: i64, amount: i32) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO duel_elo (chat_id, tg_id, pidor_elo) VALUES ($1, $2, $3)
         ON CONFLICT (chat_id, tg_id) DO UPDATE SET pidor_elo = duel_elo.pidor_elo + EXCLUDED.pidor_elo",
    )
    .bind(chat_id)
    .bind(tg_id)
    .bind(amount)
    .execute(pool)
    .await?;
    Ok(())
}

/// Add points to huya_elo (awarded for HuyActa fight wins).
pub async fn add_huya_elo(pool: &PgPool, chat_id: i64, tg_id: i64, amount: i32) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO duel_elo (chat_id, tg_id, huya_elo) VALUES ($1, $2, $3)
         ON CONFLICT (chat_id, tg_id) DO UPDATE SET huya_elo = duel_elo.huya_elo + EXCLUDED.huya_elo",
    )
    .bind(chat_id)
    .bind(tg_id)
    .bind(amount)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_duel_leaderboard(pool: &PgPool, chat_id: i64, limit: i64) -> Result<Vec<DuelElo>, AppError> {
    let rows = sqlx::query_as::<_, DuelElo>(
        &format!("SELECT {ELO_SELECT} FROM duel_elo
         WHERE chat_id = $1 AND (wins > 0 OR losses > 0)
         ORDER BY (elo + pidor_elo + huya_elo) DESC LIMIT $2"),
    )
    .bind(chat_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Returns total ELO tier label (11 tiers, 0..10000+).
pub fn elo_rank(total_elo: i32) -> &'static str {
    let key = match total_elo {
        ..500 => "duel.elo.dirt",
        500..800 => "duel.elo.bronze",
        800..1200 => "duel.elo.silver",
        1200..1600 => "duel.elo.gold",
        1600..2500 => "duel.elo.diamond",
        2500..4000 => "duel.elo.master",
        4000..5500 => "duel.elo.legend",
        5500..7000 => "duel.elo.elite",
        7000..9000 => "duel.elo.dragon",
        9000..10000 => "duel.elo.phoenix",
        _ => "duel.elo.eternal",
    };
    crate::i18n::LOCALE.t("ru", key)
}

// ── Mini-game state ──────────────────────────────────────

/// Set game_state JSON for a duel (used by dice/coin/rps mini-games).
pub async fn set_game_state(pool: &PgPool, duel_id: i32, state: serde_json::Value) -> Result<(), AppError> {
    sqlx::query("UPDATE duel_game SET game_state = $1 WHERE id = $2")
        .bind(state)
        .bind(duel_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Set game_type when accepting duel.
pub async fn set_game_type(pool: &PgPool, duel_id: i32, game_type: &str) -> Result<(), AppError> {
    sqlx::query("UPDATE duel_game SET game_type = $1 WHERE id = $2")
        .bind(game_type)
        .bind(duel_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Resolve tguser.id from tg_id for achievements.
pub async fn user_id_by_tg_id(pool: &PgPool, tg_id: i64) -> Result<Option<i32>, AppError> {
    let row: Option<(i32,)> = sqlx::query_as("SELECT id FROM tguser WHERE tg_id = $1")
        .bind(tg_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.0))
}
