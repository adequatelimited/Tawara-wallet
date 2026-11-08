# Bundled fonts

The application's typefaces, as unmodified static TrueType files. Each family
is distributed under the SIL Open Font License 1.1; its licence is the
`*-OFL.txt` file beside it, and the third-party notices list it.

Sources:

| file | family, weight | from |
|---|---|---|
| `Poppins-Regular.ttf`, `Poppins-Medium.ttf`, `Poppins-SemiBold.ttf` | Poppins 400, 500, 600 (version 4.004) | `https://raw.githubusercontent.com/google/fonts/main/ofl/poppins/` |
| `IBMPlexMono-Regular.ttf`, `IBMPlexMono-Medium.ttf` | IBM Plex Mono 400, 500 (version 2.3) | `https://raw.githubusercontent.com/google/fonts/main/ofl/ibmplexmono/` |
| `Montserrat-Medium.ttf`, `Montserrat-SemiBold.ttf`, `Montserrat-Bold.ttf`, `Montserrat-MediumItalic.ttf`, `Montserrat-BoldItalic.ttf` | Montserrat 500, 600, 700, 500 italic, 700 italic (version 9.000) | `https://raw.githubusercontent.com/JulietaUla/Montserrat/master/fonts/ttf/` |

The licences come from the same places: `ofl/poppins/OFL.txt`,
`ofl/ibmplexmono/OFL.txt`, and the Montserrat repository's `OFL.txt`. The
IBM Plex licence was published with CRLF line endings and is committed with
LF (`.gitattributes`); its text is unchanged.
Montserrat is taken from its own repository because Google Fonts ships it
only as a variable font, which iced would draw at its default Thin instance.

SHA-256 of each file as committed:

```
a9b4c49bb299e05b5f6c481e7fb5e78943d2793249a0c8874ab574a2d1ea6755  IBMPlexMono-Medium.ttf
6a3412f058c7d8dfd9170c41e85ade48e5156ecb89356110ca57a0a27734af46  IBMPlexMono-Regular.ttf
bc6e854971cea46b463be6f9eef4d9cd52f51cfc1fc0dd90c9d3e6483dc0ec61  Montserrat-Bold.ttf
b4c121b337aaa977d711b6f397c5b9672d720af622555f1e3f9d3b87f2983665  Montserrat-BoldItalic.ttf
dae47428bb041f9716604e0e07b5b0c8585b3bdd8183362f75c69fe7bb3cfaf4  Montserrat-Medium.ttf
b544641c1fde7c5228a26ddade08870a9c1dc7426446828fc8aa8acab2521a45  Montserrat-MediumItalic.ttf
b4e1563393d73fdff491a869441245aef31add2ec03d9c97a6dae4de07c52fd0  Montserrat-SemiBold.ttf
90373e7d838d32468438fc3e152dca0bdb12edcab99ea639f158790b1ba1fd05  Poppins-Medium.ttf
7e65201e9b79159e2300267cc885e16c8dcef2424cdfa09a29bfb0980a94a7ba  Poppins-Regular.ttf
d3bf1bdaf0550e83da9ac0b1d1d9fe6db086835a83aa28578e609a394b9a0286  Poppins-SemiBold.ttf
37784b44044a4ffd9256702b7c0982c37e5c8887ba90c6dca0479aea93dc898d  IBMPlexMono-OFL.txt
8b7141c03fa4f8d44e6345d5d4931709290f0f67875e452e95ac1fd3a027802e  Montserrat-OFL.txt
6be04893d770899a015649c7aa3b582f871b272f8747a92b78b17c3e5c8b2573  Poppins-OFL.txt
```
