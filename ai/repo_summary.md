# Blinky — AI Directory & Developer Guides

Welcome to the AI integration and developer documentation directory for **Blinky**. 

This directory contains comprehensive guides designed to ramp up human developers quickly and instruct offline AI coding agents on the system architecture, API interfaces, coordinate mapping formulas, matching heuristics, and voice integrations used throughout the codebase.

---

## 📖 Available Guides

Select one of the specialized guides below to inspect a component or concept:

| Guide | Description | Target Audience |
| :--- | :--- | :--- |
| 🏗️ **[System Architecture](file:///c:/projects/Jarvis/ai/architecture.md)** | Multi-process models, high-level system flows, sequence diagrams, and IPC protocols. | Architects, System Integrators, AI Agents |
| 📐 **[Coordinate Scaling & Normalization](file:///c:/projects/Jarvis/ai/coordinate_scaling.md)** | Calculations mapping physical screens to downsampled screenshots and web view CSS layout coordinates. | Developers, QA, AI Agents |
| 🎯 **[Matching Heuristics & Deduplication](file:///c:/projects/Jarvis/ai/matching_heuristics.md)** | Fuzzy target matching algorithms, context scoring bonuses, coordinate grids, and steps post-processing. | Developers, AI Agents |
| 🧠 **[AI Inference & Prompts](file:///c:/projects/Jarvis/ai/ai_inference.md)** | Chat engine prompts, preflight classification, continuation logic, single-step enforcement, and Ollama/Groq configs. | Prompt Engineers, AI Agents |
| 🗣️ **[Sarvam AI Voice Integration](file:///c:/projects/Jarvis/ai/sarvam.md)** | Bulbul (TTS) and Saaras (STT) payload formats, authentication headers, error messages, and frontend hooks. | Audio Engineers, AI Agents |
| 📝 **[Per-File API Reference](file:///c:/projects/Jarvis/ai/detailed_summaries.md)** | Detailed function signatures, classes, arguments, and module-level responsibilities. | Developers, AI Agents |
| 🗂️ **[Files Index](file:///c:/projects/Jarvis/ai/files_index.json)** | Machine-readable JSON listing of core codebase assets and their functional descriptions. | AI Agents, Automations |

---

## 🚀 Quick Repository Overview

```text
  c:\projects\Jarvis  (Blinky Project Root)
   ├── ai/                      ──► AI Documentation Hub (this folder)
   ├── frontend/src/            ──► React TypeScript GUI and Canvas viewports
   ├── src-tauri/src/           ──► Rust Native Core & Mouse Click hooks
   ├── python/                  ──► Capture, OCR Extraction & Targets Fuzzy Matching
   └── tmp/captures/            ──► Captured Telemetry Screen Buffers (temporary)
```

* **Purpose**: Privacy-first, local AI-powered tutor that captures screen states, extracts visible UI controls, runs coordinate-aware fuzzy matching, and places graphical click-target overlays on screen — one step at a time.
* **Core Tech Stack**: 
  * **Tauri (v2) + Rust**: OS-level hooks, shortcuts, window controllers, capture exclusion (`WDA_EXCLUDEFROMCAPTURE`).
  * **React + TypeScript**: Form inputs, dynamic height rendering, canvas overlay graphics.
  * **Python 3.11**: Screen captures (`dxcam`), WinRT OCR / EasyOCR, UI elements extraction (`pywinauto`).
  * **LLM Intelligence**: Local Ollama (`gemma4:e4b`) or cloud-hosted Groq Vision API (`llama-4-scout`).

---

## 🛠️ Rapid Dev Commands

Set up Blinky locally using the following commands:

```powershell
# 1. Install standard dependencies
bun install

# 2. Configure Python virtual environments and pull EasyOCR
bun run setup:python

# 3. Pull default local AI models
ollama pull gemma4:e4b

# 4. Start the application in development mode
bun run dev
```

*For details on configuring `.env` variables and custom shortcut hotkeys, please refer to the [System Architecture Guide](file:///c:/projects/Jarvis/ai/architecture.md#6-environment--settings-variables).*
