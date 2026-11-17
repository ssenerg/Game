//! Entering moves: typed, in SAN or UCI and read as the player types, or
//! picked from square to square.

use chess_core::moves::Move;
use chess_core::piece::{self, Piece};
use chess_core::position::Position;
use chess_core::san;
use chess_core::square::Square;

/// Most a typed move can be, as in `exd8=Q+`, with room to spare.
pub const MAX_TYPED: usize = 10;

/// Whether `c` can be part of a typed move.
pub fn typable(c: char) -> bool {
    c.is_ascii_alphanumeric() || "x+#=-".contains(c)
}

/// Typed text, made comparable: lower case, `0` as `o`, and without
/// captures, checks, `=` and dashes.
fn squeeze(text: &str) -> String {
    text.chars()
        .filter(|c| !"x+#=-!? ".contains(*c))
        .map(|c| {
            if c == '0' {
                'o'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect()
}

/// The legal moves `text` could be the start of, in SAN or UCI.
pub fn candidates(position: &Position, text: &str) -> Vec<Move> {
    let typed = squeeze(text);
    if typed.is_empty() {
        return Vec::new();
    }
    position
        .legal_moves()
        .into_iter()
        .filter(|&mv| {
            let san = san::format(position, mv).map(|s| squeeze(&s));
            mv.to_string().starts_with(&typed) || san.is_some_and(|s| s.starts_with(&typed))
        })
        .collect()
}

/// The legal moves from `from` to `to`: one, or a promotion's four.
pub fn between(position: &Position, from: Square, to: Square) -> Vec<Move> {
    position
        .legal_moves()
        .into_iter()
        .filter(|m| m.from == from && m.to == to)
        .collect()
}

/// Where the piece at `from` goes when put down on `at`: a king on its
/// own square put down on its own rook castles with it, as long as it
/// still may some day; anything else goes to `at` itself.
pub fn landing(position: &Position, from: Square, at: Square) -> Square {
    let Some((color, Piece::King)) = position.piece_at(from) else {
        return at;
    };
    let home = if color == piece::Color::White { 0 } else { 7 };
    let rook = position.piece_at(at) == Some((color, Piece::Rook));
    if from != Square::new(4, home).expect("e1 and e8 are squares") || !rook || at.rank() != home {
        return at;
    }
    let (short, long) = position.castling_rights(color);
    match at.file() {
        7 if short => Square::new(6, home).unwrap_or(at),
        0 if long => Square::new(2, home).unwrap_or(at),
        _ => at,
    }
}

/// Where the piece at `from` can go now, its side to move: its legal
/// moves' squares, and for castling its rook's square too.
pub fn destinations(position: &Position, from: Square) -> Vec<Square> {
    let mut to = Vec::new();
    for mv in position
        .legal_moves()
        .into_iter()
        .filter(|m| m.from == from)
    {
        if !to.contains(&mv.to) {
            to.push(mv.to);
        }
    }
    for (castles, corner) in castlings(position, from) {
        if to.contains(&castles) {
            to.push(corner);
        }
    }
    to
}

/// Where the piece at `from` could go on a later turn, as a premove: every
/// square its moves reach on an empty board, its own pieces' too, which
/// may be taken by then; a pawn's two captures, whatever stands there now;
/// and castling, while the king and its rook still may.
pub fn premove_destinations(position: &Position, from: Square) -> Vec<Square> {
    let Some((color, piece)) = position.piece_at(from) else {
        return Vec::new();
    };
    let (file, rank) = (from.file() as i8, from.rank() as i8);
    let at = |df: i8, dr: i8| {
        let (f, r) = (file + df, rank + dr);
        if (0..8).contains(&f) && (0..8).contains(&r) {
            Square::new(f as u8, r as u8)
        } else {
            None
        }
    };
    const KNIGHT: [(i8, i8); 8] = [
        (-2, -1),
        (-2, 1),
        (-1, -2),
        (-1, 2),
        (1, -2),
        (1, 2),
        (2, -1),
        (2, 1),
    ];
    const ROOK: [(i8, i8); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    const BISHOP: [(i8, i8); 4] = [(-1, -1), (-1, 1), (1, -1), (1, 1)];
    let rays = |dirs: &[(i8, i8)]| -> Vec<Square> {
        dirs.iter()
            .flat_map(|&(df, dr)| (1..8).map_while(move |n| at(df * n, dr * n)))
            .collect()
    };
    let mut to: Vec<Square> = match piece {
        Piece::Pawn => {
            let (ahead, start) = match color {
                piece::Color::White => (1, 1),
                piece::Color::Black => (-1, 6),
            };
            let mut to = vec![at(0, ahead), at(-1, ahead), at(1, ahead)];
            if rank == start {
                to.push(at(0, 2 * ahead));
            }
            to.into_iter().flatten().collect()
        }
        Piece::Knight => KNIGHT.iter().filter_map(|&(df, dr)| at(df, dr)).collect(),
        Piece::Bishop => rays(&BISHOP),
        Piece::Rook => rays(&ROOK),
        Piece::Queen => rays(&[ROOK, BISHOP].concat()),
        // A step any way a rook or bishop goes.
        Piece::King => [ROOK, BISHOP]
            .concat()
            .into_iter()
            .filter_map(|(df, dr)| at(df, dr))
            .collect(),
    };
    for (castles, corner) in castlings(position, from) {
        for sq in [castles, corner] {
            if !to.contains(&sq) {
                to.push(sq);
            }
        }
    }
    to
}

/// Where the king at `from` would castle to, with the corner it castles
/// with, for each way it still may some day; none for any other piece.
fn castlings(position: &Position, from: Square) -> Vec<(Square, Square)> {
    [7, 0]
        .into_iter()
        .filter_map(|file| Square::new(file, from.rank()))
        .map(|corner| (landing(position, from, corner), corner))
        .filter(|(castles, corner)| castles != corner)
        .collect()
}

/// Whether `from` to `to` is a pawn reaching the last rank.
pub fn promotes(position: &Position, from: Square, to: Square) -> bool {
    position
        .piece_at(from)
        .is_some_and(|(_, p)| p == Piece::Pawn)
        && (to.rank() == 0 || to.rank() == 7)
}

/// What entering a move does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entered {
    Play(Move),
    /// A pawn reaches the last rank, and the player picks what it becomes.
    Promote {
        from: Square,
        to: Square,
    },
    /// Why nothing was played.
    Refused(&'static str),
}

/// What Enter does with `text` typed: plays the move it names, or the only
/// move it could be the start of.
pub fn enter(position: &Position, text: &str, auto_queen: bool) -> Entered {
    if let Ok(mv) = san::parse(position, text) {
        return Entered::Play(mv);
    }
    let found = candidates(position, text);
    match choose(&found, auto_queen) {
        Some(entered) => entered,
        None if found.is_empty() => Entered::Refused("That isn't a legal move here."),
        None => Entered::Refused("Which one? Type more of it."),
    }
}

/// The move `moves` leave no doubt about: the only one, or a promotion
/// that's still to be chosen, unless `auto_queen` chooses it.
pub fn choose(moves: &[Move], auto_queen: bool) -> Option<Entered> {
    let &first = moves.first()?;
    if moves.len() == 1 {
        return Some(Entered::Play(first));
    }
    let promotion = moves
        .iter()
        .all(|m| m.from == first.from && m.to == first.to && m.promotion.is_some());
    if !promotion {
        return None;
    }
    if auto_queen {
        let queen = moves.iter().find(|m| m.promotion == Some(Piece::Queen))?;
        return Some(Entered::Play(*queen));
    }
    Some(Entered::Promote {
        from: first.from,
        to: first.to,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(fen: &str) -> Position {
        Position::from_fen(fen).unwrap()
    }

    fn sq(name: &str) -> Square {
        Square::parse(name).unwrap()
    }

    #[test]
    fn typing_narrows_the_moves_down() {
        let start = Position::start();
        assert_eq!(candidates(&start, "").len(), 0);
        assert_eq!(candidates(&start, "N").len(), 4);
        let nf: Vec<String> = candidates(&start, "nf")
            .iter()
            .map(|m| m.to_string())
            .collect();
        assert_eq!(nf, ["g1f3"]);
        assert_eq!(candidates(&start, "e2").len(), 2, "UCI");
        assert_eq!(
            enter(&start, "Nf", false),
            Entered::Play(Move::parse_uci("g1f3").unwrap())
        );
        assert_eq!(
            enter(&start, "e4", false),
            Entered::Play(Move::parse_uci("e2e4").unwrap())
        );
        assert!(matches!(enter(&start, "N", false), Entered::Refused(_)));
        assert!(matches!(enter(&start, "Qh5", false), Entered::Refused(_)));
    }

    #[test]
    fn castling_reads_either_way() {
        let castle = at("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        let short = Move::parse_uci("e1g1").unwrap();
        assert_eq!(enter(&castle, "0-0", false), Entered::Play(short));
        assert_eq!(enter(&castle, "O-O", false), Entered::Play(short));
        assert_eq!(candidates(&castle, "o-o").len(), 2, "short and long");
    }

    #[test]
    fn promotions_ask_unless_queens_are_automatic() {
        let pawn = at("8/4P3/8/8/8/8/k7/4K3 w - - 0 1");
        let promote = Entered::Promote {
            from: sq("e7"),
            to: sq("e8"),
        };
        assert_eq!(enter(&pawn, "e8", false), promote);
        assert_eq!(
            enter(&pawn, "e8", true),
            Entered::Play(Move::parse_uci("e7e8q").unwrap())
        );
        assert_eq!(
            enter(&pawn, "e8=N", false),
            Entered::Play(Move::parse_uci("e7e8n").unwrap())
        );
        assert_eq!(
            choose(&between(&pawn, sq("e7"), sq("e8")), false),
            Some(promote)
        );
        assert_eq!(between(&pawn, sq("e7"), sq("d8")), []);
    }

    fn squares(found: &[Square]) -> Vec<String> {
        let mut names: Vec<String> = found.iter().map(|s| s.to_string()).collect();
        names.sort();
        names
    }

    #[test]
    fn a_king_put_down_on_its_rook_castles() {
        let castle = at("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        assert_eq!(landing(&castle, sq("e1"), sq("h1")), sq("g1"));
        assert_eq!(landing(&castle, sq("e1"), sq("a1")), sq("c1"));
        assert_eq!(landing(&castle, sq("e8"), sq("h8")), sq("g8"), "black's");
        assert_eq!(landing(&castle, sq("e1"), sq("e2")), sq("e2"));
        assert_eq!(landing(&castle, sq("a1"), sq("e1")), sq("e1"), "not a king");
        assert_eq!(
            squares(&destinations(&castle, sq("e1"))),
            ["a1", "c1", "d1", "d2", "e2", "f1", "f2", "g1", "h1"]
        );
        // Not once the rook has moved.
        let moved = at("r3k2r/8/8/8/8/8/8/R3K1R1 w Qkq - 0 1");
        assert_eq!(landing(&moved, sq("e1"), sq("g1")), sq("g1"));
        assert_eq!(landing(&moved, sq("e1"), sq("h1")), sq("h1"));
    }

    #[test]
    fn premoves_reach_past_pieces_and_onto_your_own() {
        let start = Position::start();
        // Black's, while white thinks: forward, both captures, and two
        // from the start.
        assert_eq!(
            squares(&premove_destinations(&start, sq("e7"))),
            ["d6", "e5", "e6", "f6"]
        );
        assert_eq!(
            squares(&premove_destinations(&start, sq("g8"))),
            ["e7", "f6", "h6"],
            "e7 is his own pawn, taken back there"
        );
        // A bishop sees through its own pawns, to the edge.
        assert_eq!(
            squares(&premove_destinations(&start, sq("c8"))),
            ["a6", "b7", "d7", "e6", "f5", "g4", "h3"]
        );
        let rook = premove_destinations(&start, sq("a8"));
        assert_eq!(rook.len(), 14);
        assert!(rook.contains(&sq("a1")), "all the way down");
        // A king castles, onto its rook too, while it still may.
        let castle = at("r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1");
        assert_eq!(
            squares(&premove_destinations(&castle, sq("e1"))),
            ["a1", "c1", "d1", "d2", "e2", "f1", "f2", "g1", "h1"]
        );
        let none = at("r3k2r/8/8/8/8/8/8/R3K2R b kq - 0 1");
        assert_eq!(premove_destinations(&none, sq("e1")).len(), 5);
        assert_eq!(premove_destinations(&start, sq("e4")), [], "nothing there");
        assert!(promotes(
            &at("8/4P3/8/8/8/8/k7/4K3 b - - 0 1"),
            sq("e7"),
            sq("e8")
        ));
        assert!(!promotes(&start, sq("e2"), sq("e4")));
    }
}
