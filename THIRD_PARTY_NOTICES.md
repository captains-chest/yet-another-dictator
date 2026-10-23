# Third-party notices

Dictator is MIT licensed (see `LICENSE`). The app ships the following third-party pieces,
all under licenses compatible with that. This file is bundled in `Dictator.app/Contents/Resources`.

| Component | Used for | License | Source |
| --- | --- | --- | --- |
| WhisperKit 1.1.1 | On-device speech recognition (Swift code linked into the app) | MIT, © 2024 argmax, inc. | https://github.com/argmaxinc/WhisperKit |
| WhisperKit Core ML model `openai_whisper-large-v3-v20240930_turbo` | Speech model, downloaded on first launch (Core ML conversion) | MIT (argmax, inc.) | https://huggingface.co/argmaxinc/whisperkit-coreml |
| Whisper Large v3 Turbo weights and tokenizer | The original model the conversion is made from; the tokenizer files are downloaded as is | MIT, © 2022 OpenAI | https://huggingface.co/openai/whisper-large-v3-turbo, https://github.com/openai/whisper |
| FluidAudio 0.17.7 | Runs the Parakeet model on Core ML (Swift code linked into the app; the optional NemoTextProcessing binary is turned off) | Apache 2.0, © FluidInference | https://github.com/FluidInference/FluidAudio |
| NVIDIA Parakeet TDT 0.6B v2 (Core ML conversion `FluidInference/parakeet-tdt-0.6b-v2-coreml`) | English speech model, downloaded only if you choose it | CC-BY-4.0, © NVIDIA Corporation; converted to Core ML by FluidInference | https://huggingface.co/nvidia/parakeet-tdt-0.6b-v2, https://huggingface.co/FluidInference/parakeet-tdt-0.6b-v2-coreml |
| llama.cpp b11838 (official `llama.xcframework`) | Runs the optional S1-mini cleanup model on Metal (dynamic framework embedded in the app) | MIT, © 2023-2026 The ggml authors | https://github.com/ggml-org/llama.cpp |
| S1-mini by Superwhisper (`superwhisper/s1-mini-GGUF`, Q4_K_M) | Optional transcript cleanup model, downloaded only if you turn cleanup on | Apache 2.0 with an additional naming term, © 2026 Superwhisper; a derivative of Qwen3-0.6B, © 2024 Alibaba Cloud, Apache 2.0 | https://huggingface.co/superwhisper/s1-mini-GGUF |
| DictationCore 0.1.0 | Shared dictation logic | MIT, © 2026 CaptainDuckay | https://github.com/captains-chest/DictationCore |

`swift-argument-parser` (Apache 2.0) appears in `Package.resolved` because WhisperKit's command-line
tool needs it; the app doesn't link or ship it.

**Attribution (CC-BY-4.0):** "parakeet-tdt-0.6b-v2" by NVIDIA Corporation, licensed under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Source:
https://huggingface.co/nvidia/parakeet-tdt-0.6b-v2. Modified: converted to Core ML by
FluidInference (https://huggingface.co/FluidInference/parakeet-tdt-0.6b-v2-coreml); Dictator
downloads that conversion unchanged.

FluidAudio is licensed under the Apache License 2.0 (https://www.apache.org/licenses/LICENSE-2.0).

**S1-mini by Superwhisper** is licensed under the Apache License 2.0
(https://www.apache.org/licenses/LICENSE-2.0) with this additional term, which Dictator follows
by always naming it "S1-mini by Superwhisper":

> ADDITIONAL TERM
>
> In addition to the terms of the Apache License, Version 2.0 above: any use, distribution, or
> integration of this model, whether unmodified or as part of a derivative work or product, must
> continue to identify it by its original name, "S1-mini" by "Superwhisper", using that exact
> capitalization, regardless of any other name under which the model or a product incorporating
> it is marketed or distributed.

Its NOTICE file:

> S1-mini-GGUF
> Copyright 2026 Superwhisper
>
> GGUF conversions of S1-mini, which is itself a derivative of Qwen3-0.6B,
> Copyright 2024 Alibaba Cloud, licensed under the Apache License, Version 2.0.
>
> Any use, distribution, or integration of this model, whether unmodified or
> as part of a derivative work or product, must continue to identify it by its
> original name, "S1-mini" by "Superwhisper", using that exact capitalization.
> This restates the ADDITIONAL TERM of the LICENSE file, which is the
> operative text.

The full LICENSE and NOTICE are bundled in `Dictator.app/Contents/Resources/ThirdParty`
(`S1-mini-LICENSE`, `S1-mini-NOTICE`) and are downloaded with the model. Dictator downloads the
model unchanged. llama.cpp's licence is bundled there too (`llama.cpp-LICENSE`).

The full MIT license text applies to each MIT component, with that component's copyright notice:

> Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
> associated documentation files (the "Software"), to deal in the Software without restriction,
> including without limitation the rights to use, copy, modify, merge, publish, distribute,
> sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions: The above copyright notice and this
> permission notice shall be included in all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
> BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
> NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
> DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT
> OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
