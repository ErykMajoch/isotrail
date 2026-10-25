# Assets

## Palettes

`palettes/` holds colour palettes in the [Lospec](https://lospec.com/palette-list) `.hex` format: one `RRGGBB` colour per line. They are built into the `isotrail` binary and chosen with `--palette NAME`.

| Name | Colours | Author | Source |
|---|---|---|---|
| `apollo` | 46 | AdamCYounis | <https://lospec.com/palette-list/apollo> |
| `resurrect-64` | 64 | Kerrie Lake | <https://lospec.com/palette-list/resurrect-64> |
| `aap-64` | 64 | Adigun A. Polack | <https://lospec.com/palette-list/aap-64> |

The palettes are published on Lospec for anyone to use in their artwork, without a formal licence. They are included here unchanged, with thanks to their authors.

## Sprites

`sprites/` holds the route markers, drawn pixel by pixel for isotrail and covered by the project's MIT licence: `start.png` and `finish.png` are the flags at either end of the route, and `head.png` marks how far the route has been drawn. Their colours are matched to the palette when a frame is quantised.
