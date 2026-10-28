# Social media fetching

Social URLs are recognized automatically by the Rust library, CLI, MCP server,
and Python bindings. No API keys or extra configuration are needed for these
public metadata fetchers.

| Platform | Supported URLs | Result format |
| --- | --- | --- |
| TikTok | `https://www.tiktok.com/@user/video/123` | `tiktok_video` |
| YouTube | `/watch?v=ID`, `/shorts/ID`, `/live/ID`, `/embed/ID`, `youtu.be/ID`, `youtube-nocookie.com/embed/ID` | `youtube_video` |
| Vimeo | `https://vimeo.com/123`, `https://player.vimeo.com/video/123` | `vimeo_video` |
| SoundCloud | `https://soundcloud.com/user/track` | `soundcloud_track` |
| SoundCloud playlists | `https://soundcloud.com/user/sets/playlist` | `soundcloud_playlist` |
| X / Twitter (existing) | `https://x.com/user/status/123`, `twitter.com/user/status/123` | `twitter_tweet` |

```sh
fetchkit fetch 'https://www.tiktok.com/@scout2015/video/6718335390845095173'
fetchkit fetch 'https://www.youtube.com/shorts/jNQXAC9IVRw'
fetchkit fetch 'https://vimeo.com/863362136'
fetchkit fetch 'https://soundcloud.com/forss/flickermood'
```

TikTok, Vimeo and SoundCloud return Markdown containing the public title, author,
description and thumbnail URL when supplied by the provider. Their responses are
explicitly labeled **public metadata only**. No video/audio is downloaded or
transcribed. YouTube separately attempts English captions; they are often
unavailable. X/Twitter retains its existing post-content fetcher.

TikTok's current embed API can omit title/author fields. In that case the result
contains the platform and canonical video URL, without invented content. Embed
HTML and player scripts are ignored. Provider errors (including private/deleted
content, rate limits and redirects) retain their HTTP status and return an error
instead of fabricated metadata. Invalid or oversized JSON returns a fetch error.

Use canonical public links. TikTok short links (`vm.tiktok.com`, `vt.tiktok.com`),
profiles, private sharing URLs, and other social platforms use the default HTML
fetcher; useful content is not guaranteed. Login walls and access restrictions
are not bypassed. Tracking queries are removed from URLs sent to oEmbed.

URL policies must allow both the supplied page URL and the provider endpoint:

- TikTok: `https://www.tiktok.com/oembed`
- YouTube: `https://www.youtube.com/oembed` and, for optional captions, `/api/timedtext`
- Vimeo: `https://vimeo.com/api/oembed.json`
- SoundCloud: `https://soundcloud.com/oembed`

The new oEmbed fetcher does not follow API redirects or retrieve returned image,
author, or player URLs. Existing DNS restrictions, body limits and URL policy
apply. Rendered output is bounded and reports `size` and `truncated`.

Provider references: [TikTok](https://developers.tiktok.com/docs/en/embed-videos),
[Vimeo](https://developer.vimeo.com/api/oembed/videos),
[SoundCloud](https://developers.soundcloud.com/docs/oembed).
