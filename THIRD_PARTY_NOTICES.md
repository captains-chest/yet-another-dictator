# Third-party notices

Dictator is MIT licensed (see `LICENSE`). The app ships the following third-party pieces,
all under licenses compatible with that. This file is bundled in `Dictator.app/Contents/Resources`.

| Component | Used for | License | Source |
| --- | --- | --- | --- |
| WhisperKit 1.1.1 | On-device speech recognition (Swift code linked into the app) | MIT, © 2024 argmax, inc. | https://github.com/argmaxinc/WhisperKit |
| WhisperKit Core ML model `openai_whisper-large-v3-v20240930_turbo` | Speech model, downloaded on first launch (Core ML conversion) | MIT (argmax, inc.) | https://huggingface.co/argmaxinc/whisperkit-coreml |
| Whisper Large v3 Turbo weights and tokenizer | The original model the conversion is made from; the tokenizer files are downloaded as is | MIT, © 2022 OpenAI | https://huggingface.co/openai/whisper-large-v3-turbo, https://github.com/openai/whisper |
| FluidAudio 0.17.7 | Runs the Parakeet and Phonon-2 models on Core ML (Swift code linked into the app; the optional NemoTextProcessing binary is turned off) | Apache 2.0, © FluidInference | https://github.com/FluidInference/FluidAudio |
| NVIDIA Parakeet TDT 0.6B v2 (Core ML conversion `FluidInference/parakeet-tdt-0.6b-v2-coreml`) | English speech model, downloaded only if you choose it | CC-BY-4.0, © NVIDIA Corporation; converted to Core ML by FluidInference | https://huggingface.co/nvidia/parakeet-tdt-0.6b-v2, https://huggingface.co/FluidInference/parakeet-tdt-0.6b-v2-coreml |
| Phonon-2 (Core ML conversion `FluidInference/phonon-2-coreml`) | English speech model, downloaded only if you choose it | CC-BY-4.0, © 2026 Fermion Research, derived from NVIDIA Parakeet TDT 0.6B v3 (© NVIDIA Corporation, CC-BY-4.0); converted to Core ML by FluidInference | https://huggingface.co/FermionResearch/Phonon-2, https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3, https://huggingface.co/FluidInference/phonon-2-coreml |
| DictationCore 0.1.0 | Shared dictation logic | MIT, © 2026 CaptainDuckay | https://github.com/captains-chest/DictationCore |

`swift-argument-parser` (Apache 2.0) appears in `Package.resolved` because WhisperKit's command-line
tool needs it; the app doesn't link or ship it.

**Attribution (CC-BY-4.0):** "parakeet-tdt-0.6b-v2" by NVIDIA Corporation, licensed under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Source:
https://huggingface.co/nvidia/parakeet-tdt-0.6b-v2. Modified: converted to Core ML by
FluidInference (https://huggingface.co/FluidInference/parakeet-tdt-0.6b-v2-coreml); Dictator
downloads that conversion unchanged.

**Attribution (CC-BY-4.0):** "Phonon-2" by Fermion Research, licensed under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Source:
https://huggingface.co/FermionResearch/Phonon-2. It is derived from "parakeet-tdt-0.6b-v3" by NVIDIA
Corporation (https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3), also CC BY 4.0. Changes by
Fermion Research: the encoder's linear weights were re-trained for English and quantised to a
five-value alphabet, and the remaining parameters were quantised to 6-bit tables. Converted to
Core ML by FluidInference (https://huggingface.co/FluidInference/phonon-2-coreml, revision
a812a0dfef205660787ef6234317ab79acf4d5d6); Dictator downloads that conversion unchanged.

FluidAudio is licensed under the Apache License 2.0 (https://www.apache.org/licenses/LICENSE-2.0).

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
