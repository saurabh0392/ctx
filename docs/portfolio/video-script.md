# ctx: 60-second demo video

Voiceover script plus a timed shot list, elevator-pitch style. Produce as a screen recording of the live dashboard in the light theme (it matches the redesign). This is the script and shot list, not the video file.

## Voiceover (142 words, ~2.4 words/sec)

**[Hook]** Your coding agent works in a limited context window, and most of it leaks. It pays two taxes, and you can't see either one.

**[See]** This is See. The output tax is what your agent reads back from a tool and rarely needs again. The input tax is the menu every server loads on every request, whether it's used or not. On my machine: 77,000 tokens a request, 64,000 of them never called.

**[Save]** This is Save. ctx trims a tool only after a controlled test on your own work shows the shorter version did no harm. Every tool earns it, and any cut is one command back.

**[Home]** This is Home. One number: the tokens you've reclaimed. It runs locally, nothing leaves your machine, and every change is reversible.

**[Close]** ctx. See the waste, cut what's safe, keep the thread.

## Shot list

| Time | Surface | On screen | Voiceover |
|------|---------|-----------|-----------|
| 0:00-0:08 | Home (headline) | Cold open on the ctx Home headline, "ctx makes your agent leaner without losing the thread." Slow scroll starts. Keep the reclaimed number out of frame; it is the payoff. | Hook line |
| 0:08-0:23 | See | Cut to See. Two cards side by side, Output tax and Input tax. Push in on Input tax (77K per request, 63K dead weight), then pan down the itemized bars, biggest first, green trimmable share. | Two-tax line |
| 0:23-0:38 | Save | Cut to Save. Hold on the four-stage ladder: Watching, Trial, Proving, Earned. Stop above the harm-read stats and the per-tool point numbers. Optional caption: "one command back." | Earn-it line |
| 0:38-0:53 | Home | Cut back to Home. The hero number counts up to the tokens reclaimed. Then the Trust close and the footer, "nothing leaves this machine." | Payoff plus local/reversible line |
| 0:53-1:00 | Home (wordmark) | Settle on the number and the ctx wordmark. End card. | Close line |

## Production notes

- Record in the light theme. The reclaimed number is live data from one machine, so treat it as illustrative.
- Before publishing, blur anything real the live UI exposes when a row is expanded: file paths in See's trim diffs, any ticket or client names.
- The Save beat is the one to watch. Keep the camera on the ladder and off the statistics below it. That is where the "how it decides" IP lives, and the brief is to show the earn-it proof without revealing how.
- Every beat names the surface it shows, so the four-surface product reads clearly in sixty seconds.
