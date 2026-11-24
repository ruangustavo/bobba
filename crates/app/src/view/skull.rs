use rune::{Color, El, col, el, row};

const SKULL: [&str; 9] = [
    "..#####..",
    ".#.....#.",
    "#.......#",
    "#.##.##.#",
    "#.##.##.#",
    "#.......#",
    ".#.#.#.#.",
    ".#.#.#.#.",
    "..#####..",
];

pub fn skull(pixel: f32, ink: Color) -> El {
    SKULL.iter().fold(col(), |grid, line| {
        grid.child(line.chars().fold(row(), |cells, cell| {
            let dot = el().size(pixel, pixel);
            cells.child(if cell == '#' { dot.fill(ink) } else { dot })
        }))
    })
}
