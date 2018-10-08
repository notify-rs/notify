# Acknowledgements

Many people, software, projects have helped make Notify a reality. Here I thank
and acknowledge those I can recall. Sincere apologies if you are missing!

## Co-Maintainers

- **[Daniel Faust]** helped both in code and in community at a time when I was
  drifting away from the project. Besides contributing the all-important
  debouncing interface, they answer queries, fix bugs, and keep a benevolent
  eye and presence on the project. I am certain Notify would not be standing
  today if it wasn't for them.

[Daniel Faust]: https://github.com/dfaust

## Contributors

All those whose commits made up Notify, both in the old tree that ended with
Notify v4, and in the new tree of Notify v5 onwards. [This list][gh-contrib]
has them all, but I give particular mention to:

- The old guard: **[ShuYu Wang]**, **[Jorge Israel Peña]**, **[Pierre
  Baillet]**, **[Antti Keränen]**, **[Michael Maurizi]**, **[John Quigley]**.
  They helped build up the original backends, which made this library truly
  useful.

- **[Colin Rofls]** for what was to be the new FSEvent backend and associated
  work and discussion. It ended up that the backend wasn't used in Notify (by
  default), but that in itself was an important step in figuring things out and
  refocusing.

- All who did not contribute code, but words and time and effort.

- **[Simonas Kazlauskas]**, for thoughtful comments and suggestions on the v5
  design, that I ended up regretting not following up on earlier! She even
  foresaw an application of the new design that I didn't fully realise until
  nearly a year later. Thank you! I hope you like this version better.

[gh-contrib]: https://github.com/passcod/notify/graphs/contributors
[ShuYu Wang]: https://github.com/andelf
[Jorge Israel Peña]: https://github.com/blaenk
[Pierre Baillet]: https://github.com/octplane
[Antti Keränen]: https://github.com/detegr
[Michael Maurizi]: https://github.com/maurizi
[John Quigley]: https://github.com/jmquigs
[Colin Rofls]: https://github.com/cmyr
[Simonas Kazlauskas]: https://kazlauskas.me/

## Upstreams

Notify relies on some libraries to do its work. More than that, we rely on the
_people_ maintaining and developing these libraries. I thank:

- **[Hanno Braun]**, for the excellent [inotify wrapper], as well as feedback
  on the early Notify v5 design.

- **[Peter Atashian]** for the [winapi] crate.

- **[Andrew Gallant]** for walkdir.

- **[Carl Lerche]** for Tokio and Mio.

- [Simonas Kazlauskas] again for the [libloading] library.

[Hanno Braun]: https://github.com/hannobraun
[Peter Atashian]: https://github.com/retep998
[Andrew Gallant]: https://burntsushi.net/
[Carl Lerche]: http://carllerche.com/

[inotify wrapper]: https://github.com/inotify-rs/inotify
[FSEvent wrapper]: https://github.com/octplane/fsevent-rust
[winapi]: https://github.com/retep998/winapi-rs
[libloading]: https://github.com/nagisa/rust_libloading

## Community

- All who have used Notify and Cargo Watch and told me about it!

- The nice people of the [Tokio gitter channel], for answering questions and at
  one point being a patient rubber duck.

- **[Matt Green]**, of [watchexec]. I am thankful for two reasons: firstly,
  watchexec is excellent work and I use it all the time. Cargo Watch was
  originally based directly on Notify, but it was always a struggle to make it
  robust. Thanks to watchexec, I was able to make Cargo Watch a sort of "skin"
  on top and focus on Notify instead. The second reason is that the project
  makes fairly extensive use of Notify, and some of the ways it bumped against
  the old design and worked around it informed the design of Notify v5. Seeing
  how a project is used is always very helpful.

- Another early adopter of Notify was **[mdBook]**. Yes, _that_ mdBook! The Rust
  Programming Language official books are produced using Notify. That's pretty
  neat, but also the original implementation of the mdBook `watch` subcommand
  hit an issue while using Notify that was one of the impetuses for integrating
  a debounced interface (which finally happened much later).

[Tokio gitter channel]: https://gitter.im/tokio-rs/tokio
[Matt Green]: https://github.com/mattgreen
[watchexec]: https://github.com/mattgreen/watchexec
[mdBook]: https://github.com/rust-lang-nursery/mdBook

## Tooling

- **[rtss]** has become an essential tool to investigate timings from logs.

- **[ripgrep]**, again by [Andrew Gallant].

[rtss]: https://github.com/Freaky/rtss
[ripgrep]: https://github.com/BurntSushi/ripgrep

## From afar

- **[Steve Klabnik]** and the Rust docs team, for providing inspiration and
  will to create good documentation for Notify v5 from the get go. There were
  some pretty instrumental Twitter threads that shaped my understanding and
  desires in this regard.

- **[Fiona Aeterna]** for compiler-related threads, which made me aware and
  thinking about things like instructions and cache lines, even though I
  probably don't do that enough. I then went on to learn lots about cosplay and
  semi-professional prop manufacture, but that's just bonus.

[Steve Klabnik]: https://twitter.com/steveklabnik
[Fiona Aeterna]: https://twitter.com/fioraaeterna

## Special thanks

- **[Saf]** for being a good friend and providing inspiration and motivation.
  She has had significant influence on me, and that has influenced this work in
  turn. Notify would still exist, but neither I nor it would be the same.

- **[Mako]** for their frienship and their rants about Rust, making me think
  more deeply, about the language, about the construction of software, and
  about life in general.

- **[Kat Marchán]** and the [WeAllJS] community for their lovely collaboration
  environment, the technical expertise displayed and shared, and their work
  proving that CoC enforcement does not have to be scary nor exceptional.

[Saf]: https://notsafforwork.com/
[Mako]: http://aboutmako.makopool.com/
[Kat Marchán]: https://twitter.com/maybekatz
[WeAllJS]: https://wealljs.org/
