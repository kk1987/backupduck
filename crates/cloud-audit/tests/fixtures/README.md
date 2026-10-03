# Parser fixtures

`parser/swbisb.json`, `parser/EWgK9e.json`, `parser/VrseUb.json` and
`parser/fDcn4b.json` are copied unchanged from
[xob0t/Google-Photos-Toolkit](https://github.com/xob0t/Google-Photos-Toolkit)
`tests/fixtures/parser/` at commit `6db9ab355e4797327e4078cf87a2b6994d36cf73`.

They are recorded Google Photos `batchexecute` responses, already unwrapped
from the `wrb.fr` envelope, that upstream sanitised before publishing (media
URLs, names and identifiers replaced with placeholders). `EWgK9e.json` is the
item list at `response[0][1]`, as Toolkit stores it.

Upstream licence:

```
MIT License

Copyright (c) 2024 xob0t

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
