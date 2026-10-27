# Full-SHA codeload pin comparison

This comparison downloads archives at the resolved 40-character commit SHA, records each new archive SHA-256 and byte length, and recomputes the existing selected-file manifest. It does **not** run a FITS/PDF measurement.

Previous selected-file manifest digest: `57e18a9d97d70e4c9e8fd919546c537fea79f9d30e42437c8b4d409fd027ecc5`.
Full-SHA selected-file manifest digest: `57e18a9d97d70e4c9e8fd919546c537fea79f9d30e42437c8b4d409fd027ecc5`.

| Corpus | Commit SHA | Old archive SHA-256 | New archive SHA-256 | Old→new archive bytes | Selected files / bytes | Per-file comparison |
|---|---|---|---|---:|---:|---|
| astropy | `dbbf2acd6dbb177db184bff319d2a2950dd178bb` | `9e41903218933be74870e1723c08f856a503357618631a040475a88b3ee4f58c` | `3c6b762797ea8f292d5c35fbb3ebe82b7a28139783b5c6e870a38e576d62a46f` | 6982863 → 6989587 | 34 / 691200 | exact: no missing, extra, or changed selected file |
| cfitsio | `113781d8aa442cf8d3c27295d9084674b8b039f2` | `cc5f1cc219b0de8f8d8500236b080bff13d2de12b9ed253c7a417b1b7d962509` | `d93c0de372a264156d2f3e1eab2d4159e132c35c2216f71c0af0009b92a72b5e` | 2166070 → 2169497 | 5 / 694080 | exact: no missing, extra, or changed selected file |
| drizzlepac | `635834581ea2b226a3f3028b6ac54efb5e180dfc` | `86cd9e8da51bf7e33de1ba0a844d27d3a0dbd82f730894430e3bf129f4b3b669` | `c78e1eff17a80ddfbcfc78b1ba26fc542dec14e476cca4faf8af9d496f33386e` | 14845763 → 14851757 | 123 / 15488640 | exact: no missing, extra, or changed selected file |
| reproject | `9dddd419111a9ca159baaf7d5e2eb5729b447b58` | `6b1c5b4e526d1c392b47ba2e98ec1b9fab99bae0765e110bf54ac3ef1a83f23d` | `d20377b7480f05869bb6666c01a32fd59b019f373553b13dc3feb0e7d514d222` | 1672432 → 1672777 | 16 / 1903680 | exact: no missing, extra, or changed selected file |
| pdfjs | `606bd6a4e138bbbc22fad10872d2ffa93365b6b1` | `222b1fb33e0bc09ebf125f0d4ce1f84226a6d7bdd26a0e3c1c5782583e8d3046` | `33bac2537b4a5e3d1cfd74083c4b3bcca1f0bd1646a8046526a1b1de1b2830f3` | 105076418 → 105087051 | 1009 / 124725746 | exact: no missing, extra, or changed selected file |
| qpdf | `4eba95899886e851cc41d76886483b347612f2a8` | `a93d12cd81807006ec1e949229fe72211be2d0999ce5c48d4e597b7f01eb039c` | `c6de7611d2651ac71eebbabd0132b1dc13e8def0e4e640e791190f903490c799` | 19381334 → 19383624 | 630 / 38235783 | exact: no missing, extra, or changed selected file |
| verapdf | `bb75f4f0073d9350dfd058c0162a367e6fadf25e` | `3f45b024d31ba88fca5710976c84eb2d91c56cc12d863bd2be1b0dfd2ef91f1e` | `26e44331c4cbd5bbc98662e179704b8c13334f8f439cbb8e0a46f21fc8489c1b` | 140967512 → 140980346 | 569 / 8891630 | exact: no missing, extra, or changed selected file |

The full-SHA codeload archive hashes are recorded as new baselines where their bytes differ. Equality is asserted only for the selected per-file path/size/SHA-256 manifest; archive-byte equality is not claimed. The prior qpdf manifest did not list the eleven uppercase `.Pdf` near-misses; their paths are now explicitly recorded as exclusions without adding them to the 630 selected PDFs or changing the selected-file digest. The machine-readable per-corpus details are in `fullsha-repin-comparison.json`.
