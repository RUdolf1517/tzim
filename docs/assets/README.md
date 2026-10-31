# Tzim preview assets

`tzim-preview.png` and `tzim-preview.gif` are designed infographic previews for the README and TerminalTrove, **not a simulated terminal recording**.

The figures come from the real hosted corpus report in [PR #91 comment 6545541385](https://github.com/RUdolf1517/tzim/pull/91#issuecomment-6545541385), produced by [run 41215985502](https://github.com/RUdolf1517/tzim/actions/runs/41215985502):

- 698 files; 421 optimized; 159 already optimal; 86 contract refusals; 32 expected invalid controls;
- 22,008,529 → 19,881,896 bytes; 2,126,633 bytes saved (9.66%);
- optimized formats: 340 PNG, 46 JPEG, 34 TIFF and 1 zlib;
- second pass: zero files changed.

The GIF keeps the same values and only moves a highlight between the optimized, savings and idempotence regions. It does not invent intermediate measurements.

README embedding:

```markdown
![Tzim corpus result: 421 of 698 files optimized, 9.66% saved, and zero files changed on the second pass](docs/assets/tzim-preview.gif)
```

TerminalTrove accepts the PNG as the required still preview; the GIF is the recommended animated companion. Both are 1440×900.
