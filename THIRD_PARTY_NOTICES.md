# Third-party notices

The top-level AGPL license applies to original zkip material. It does not replace
the licenses or copyright notices of third-party material.

## Succinct Labs SP1 Project Template

This repository was bootstrapped from the
[SP1 Project Template](https://github.com/succinctlabs/sp1-project-template).
Portions derived from that template are:

> Copyright (c) 2024 Succinct Labs

Those portions are available under the MIT License in [LICENSE-MIT](LICENSE-MIT).
Later zkip modifications and the combined application are distributed under the
top-level AGPL license while retaining Succinct's notice.

zkip also consumes Succinct SP1 crates from crates.io, including `sp1-zkvm`,
`sp1-sdk`, and `sp1-build`. Those dependencies are distributed by their authors
under their own licenses and are not relicensed by zkip.

## Country and regional codes

[`data/countries.csv`](data/countries.csv) is an unmodified copy of `all/all.csv`
from release v10.0 of Luke Hutton's
[ISO-3166-Countries-with-Regional-Codes](https://github.com/lukes/ISO-3166-Countries-with-Regional-Codes/tree/v10.0)
project. It is licensed under the
[Creative Commons Attribution-ShareAlike 4.0 International License](https://creativecommons.org/licenses/by-sa/4.0/).

Upstream source:
<https://github.com/lukes/ISO-3166-Countries-with-Regional-Codes/blob/v10.0/all/all.csv>

The copy committed here is unmodified and has SHA-256:
`347bba35029f804f53780062052499781d267b8a5d887bf3b051e80a68390d6d`.

## Runtime GeoIP data

The GeoIP range database is not committed to this repository. zkip downloads it
at runtime from the
[`@ip-location-db/geo-whois-asn-country`](https://github.com/sapics/ip-location-db/tree/main/geo-whois-asn-country)
dataset. That downloaded data remains governed by the terms published by its
provider and is not covered by zkip's AGPL license.
