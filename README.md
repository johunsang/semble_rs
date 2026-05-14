<!--
Keywords: code search, semantic code search, AI agent, LLM, BM25, embeddings,
          tree-sitter, AST, dependency graph, impact analysis, Rust, CLI,
          Claude Code, Codex, Cursor, grep replacement, token reduction,
          potion-code, model2vec, hybrid search, RRF, korean code search,
          한글 코드 검색, AI 코드 검색
-->

# semble_rs

> **Fast, accurate, AI-agent-native code search written in Rust.**
> A drop-in replacement for `grep`, `cat`, `read`, and `ls` when LLMs explore code —
> one hybrid (BM25 + semantic) search returns ranked snippets with line numbers,
> so the agent never reads a whole file again.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-blue.svg)](#installation)
[![Languages](https://img.shields.io/badge/languages-Rust%20%7C%20Python%20%7C%20TS%20%7C%20Go%20%7C%20Java%20%7C%20C%2FC%2B%2B-green.svg)](#지원-언어)
[![Token savings](https://img.shields.io/badge/token%20savings-up%20to%20--93%25-brightgreen.svg)](#토큰-절감-비교-semble_rs-vs-rtk-vs-grep-vs-cat)

**Keywords:** AI code search · semantic code search · LLM agent tools · grep replacement · BM25 + embeddings · Tree-sitter AST · dependency graph · impact analysis · Rust CLI · Claude Code · Codex · Cursor · 한글 코드 검색

---

**grep, cat, read, ls를 대체하는 AI 에이전트용 코드 검색.**  
검색 한 번으로 관련 코드를 찾아서, 파일을 하나씩 읽을 필요를 없앱니다.

```
기존:  ls → grep → cat file1 → cat file2 → ...  (58,000 토큰/세션)
semble_rs:  search --compact                     ( 4,000 토큰/세션, -93%)
```

| 기존 도구 | 하는 일 | semble_rs가 대체하는 방법 |
|---|---|---|
| `grep -rn` | 키워드 검색 | `--compact`가 같은 크기 출력 + **semantic search** |
| `cat / read` | 파일 내용 읽기 | 검색 결과에 매칭 라인 포함 → **읽을 필요 없음** |
| `ls / find` | 파일 구조 탐색 | 검색 결과에 파일 경로 포함 → **탐색할 필요 없음** |

원본 [semble](https://github.com/MinishLab/semble) (Python)을 Rust로 재작성하고, 의존성 분석 기능을 추가한 프로젝트.

**한글 검색 지원** — BM25 토크나이저가 유니코드(`\p{L}`)를 지원하여 한글 주석, 문서, 변수명도 키워드 검색 가능. 원본 semble은 ASCII만 인식.

## 원본 semble이란

MinishLab이 만든 AI 에이전트용 코드 검색 라이브러리. 에이전트가 `grep` + `read`로 파일을 하나씩 읽는 대신, 자연어 쿼리 한 줄로 관련 코드 스니펫만 반환합니다.

**원본 semble의 핵심 구조:**

```
쿼리 → BM25(키워드) + Semantic(임베딩) → RRF 하이브리드 융합 → 스마트 랭킹 → 결과
```

**원본 semble의 특징:**

- Python (model2vec + bm25s + vicinity)
- potion-code-16M 임베딩 모델
- MCP 서버 모드 지원
- 줄 기반 청킹
- \~98% 토큰 절감 (파일 전체 읽기 대비)

## 원본 semble의 단점과 개선 사항

```
원본 semble (Python)                       semble_rs (Rust)
┌──────────────────────────┐               ┌──────────────────────────────────┐
│                          │               │                                  │
│  검색만 가능             │   ──────►     │  검색 + 의존성 분석 + 영향 분석  │
│  (search, find-related)  │               │  (search, deps, impact)          │
│                          │               │                                  │
├──────────────────────────┤               ├──────────────────────────────────┤
│                          │               │                                  │
│  줄 기반 청킹            │   ──────►     │  Tree-sitter AST 기반 청킹       │
│  (고정 길이로 잘라서     │               │  (함수/클래스/구조체 단위 분할)   │
│   함수가 중간에 잘림)    │               │                                  │
│                          │               │                                  │
├──────────────────────────┤               ├──────────────────────────────────┤
│                          │               │                                  │
│  줄번호 없음             │   ──────►     │  grep 내장 — 매칭 줄번호 + 코드  │
│  (청크 범위만 반환)      │               │  라인을 결과에 포함              │
│                          │               │                                  │
├──────────────────────────┤               ├──────────────────────────────────┤
│                          │               │                                  │
│  긴 파일 누락            │   ──────►     │  형제 청크 부스팅으로 해결       │
│  (819줄 파일의 함수를    │               │  (같은 파일의 다른 청크도        │
│   검색에서 놓침)         │               │   키워드 매칭 시 점수 올림)      │
│                          │               │                                  │
├──────────────────────────┤               ├──────────────────────────────────┤
│                          │               │                                  │
│  노이즈 제거 없음        │   ──────►     │  점수 필터 — 1위 점수의 12%      │
│  (항상 k개 전부 반환)    │               │  미만 결과 자동 제거             │
│                          │               │                                  │
├──────────────────────────┤               ├──────────────────────────────────┤
│                          │               │                                  │
│  Python (느림)           │   ──────►     │  Rust (빠름)                     │
│  pip install 필요        │               │  단일 바이너리, 의존성 없음      │
│                          │               │                                  │
├──────────────────────────┤               ├──────────────────────────────────┤
│                          │               │                                  │
│  ASCII 토큰만 인식       │   ──────►     │  유니코드 토큰 지원              │
│  ([a-zA-Z] 정규식)       │               │  (\p{L} — 한글, CJK 등)         │
│                          │               │                                  │
└──────────────────────────┘               └──────────────────────────────────┘
```

## 아이디어 도식

```
                          ┌─────────────────────────────────────────┐
                          │            semble_rs 검색 파이프라인     │
                          └─────────────────────────────────────────┘

  ┌─────────┐     ┌──────────────────────────────────────────────────────────────┐
  │  소스   │     │                    인덱싱 (매 실행)                           │
  │  코드   │────►│                                                              │
  │  파일   │     │  ┌──────────────┐  ┌──────────────┐  ┌────────────────────┐  │
  └─────────┘     │  │ Tree-sitter  │  │   임베딩     │  │   의존성 그래프    │  │
                  │  │ AST 파싱     │  │   인코딩     │  │   구축            │  │
                  │  │              │  │              │  │                    │  │
                  │  │ 함수/클래스  │  │ potion-code  │  │ import 추출       │  │
                  │  │ 단위 청킹   │  │ -16M 모델    │  │ 심볼 추출         │  │
                  │  │              │  │              │  │ 파일간 의존 해석  │  │
                  │  └──────┬───────┘  └──────┬───────┘  └────────┬───────────┘  │
                  │         │                 │                   │              │
                  │         ▼                 ▼                   ▼              │
                  │  ┌──────────────┐  ┌──────────────┐  ┌────────────────────┐  │
                  │  │  BM25 인덱스 │  │ 벡터 인덱스  │  │  파일 그래프       │  │
                  │  │  (키워드)    │  │ (시맨틱)     │  │  (deps/impact)     │  │
                  │  └──────────────┘  └──────────────┘  └────────────────────┘  │
                  └──────────────────────────────────────────────────────────────┘

  ┌─────────┐     ┌──────────────────────────────────────────────────────────────┐
  │         │     │                    검색 (매 쿼리)                            │
  │  쿼리   │────►│                                                              │
  │         │     │  ┌──────────┐  ┌──────────┐                                  │
  └─────────┘     │  │  BM25    │  │ Semantic │                                  │
                  │  │  점수    │  │  점수    │                                  │
                  │  └────┬─────┘  └────┬─────┘                                  │
                  │       │             │                                         │
                  │       ▼             ▼                                         │
                  │  ┌────────────────────────┐                                   │
                  │  │    RRF 하이브리드 융합  │                                   │
                  │  │    (alpha 가중 결합)    │                                   │
                  │  └───────────┬────────────┘                                   │
                  │              │                                                │
                  │              ▼                                                │
                  │  ┌────────────────────────┐                                   │
                  │  │    스마트 랭킹          │                                   │
                  │  │    · 정의 부스팅        │                                   │
                  │  │    · 파일 일관성 부스팅  │                                   │
                  │  │    · 형제 청크 부스팅    │  ◄── 긴 파일 누락 해결           │
                  │  │    · 그래프 부스팅       │  ◄── 의존 파일도 검색 반영       │
                  │  │    · 테스트 파일 페널티  │                                   │
                  │  └───────────┬────────────┘                                   │
                  │              │                                                │
                  │              ▼                                                │
                  │  ┌────────────────────────┐                                   │
                  │  │    노이즈 필터          │  ◄── 1위의 12% 미만 제거         │
                  │  └───────────┬────────────┘                                   │
                  │              │                                                │
                  │              ▼                                                │
                  │  ┌────────────────────────┐                                   │
                  │  │    매칭 라인 추출       │  ◄── grep 내장                   │
                  │  │    (줄번호 + 코드)      │                                   │
                  │  └───────────┬────────────┘                                   │
                  │              │                                                │
                  └──────────────┼────────────────────────────────────────────────┘
                               │
                               ▼
                  ┌──────────────────────────┐
                  │  결과                     │
                  │                          │
                  │  ## 1. src/lib/auth.ts    │
                  │    L45: function login()  │
                  │    L89: auth.signIn()     │
                  │                          │
                  │  ## 2. src/api/route.ts   │
                  │    L12: import { login }  │
                  └──────────────────────────┘


                          ┌─────────────────────────────────────────┐
                          │       의존성 분석 (원본 semble에 없는 기능)│
                          └─────────────────────────────────────────┘

  semble_rs deps src/lib/firestore.ts .

      firestore.ts
      ├── 정의: upsertUser, getUser, createPage, deletePage ...
      ├── 의존: firebase.ts
      └── 사용처: [slug]/page.tsx, api/pages/route.ts, dashboard/page.tsx ...

  semble_rs impact src/lib/firestore.ts .

      firestore.ts 변경 시 영향:
      ├── 직접 import: api/pages/route.ts, dashboard/page.tsx ...
      └── 간접 전이: api/admin/route.ts (dashboard → admin)
```

## 왜 Rust인가

원본 semble은 Python입니다. Rust로 재작성한 이유:

```
Python 런타임                              Rust 바이너리
┌──────────────────────────┐               ┌──────────────────────────────┐
│                          │               │                              │
│  pip install 필요        │               │  단일 바이너리 (~15MB)       │
│  Python 3.10+ 필수       │   ──────►     │  런타임 의존성 없음          │
│  venv 권장               │               │  어디서든 바로 실행          │
│                          │               │                              │
├──────────────────────────┤               ├──────────────────────────────┤
│                          │               │                              │
│  model2vec (PyTorch)     │               │  ndarray + safetensors       │
│  bm25s (NumPy)           │   ──────►     │  직접 구현 (외부 ML 없음)   │
│  vicinity (NumPy)        │               │  순수 Rust 행렬 연산        │
│  ~200MB+ 의존성          │               │                              │
│                          │               │                              │
├──────────────────────────┤               ├──────────────────────────────┤
│                          │               │                              │
│  GIL로 단일 스레드       │               │  네이티브 멀티스레드 가능    │
│  인터프리터 오버헤드     │   ──────►     │  컴파일 최적화              │
│                          │               │  제로 카피 문자열 처리       │
│                          │               │                              │
└──────────────────────────┘               └──────────────────────────────┘
```

AI 에이전트가 사용하는 도구는 **설치가 간단하고 빠르게 실행**되어야 합니다. pip 환경 설정 없이 바이너리 하나로 어디서든 실행되는 게 Rust의 최대 장점입니다.

## 원본 semble 로직 보존 + 추가한 로직

### 원본 semble에서 그대로 가져온 것

| 로직 | 원본 semble (Python) | Rust 포팅 |
| --- | --- | --- |
| BM25 인덱스 | `bm25s` 라이브러리 | `bm25.rs` — 직접 구현 |
| 시맨틱 임베딩 | `model2vec.StaticModel` | `encoder.rs` — safetensors 직접 로딩 |
| 벡터 검색 | `vicinity.CosineBasicBackend` | `encoder.rs` — ndarray dot product |
| RRF 하이브리드 융합 | `search.py` | `search.rs` — 동일한 1/(k+rank) 공식 |
| alpha 가중치 | `weighting.py` | `ranking/weighting.rs` — 심볼 0.3, 자연어 0.5 |
| 정의 부스팅 | `boosting.py` | `ranking/boosting.rs` — 동일 정규식 |
| 테스트 파일 페널티 | `penalties.py` | `ranking/penalties.rs` — 동일 패턴 |
| 토크나이저 | `tokens.py` | `tokens.rs` — camelCase/snake_case 분리 + **유니코드 지원** |
| 파일 워커 | `file_walker.py` | `file_walker.rs` — gitignore 존중 |

### 새로 추가한 것

**1. Tree-sitter AST 청킹** — 원본 semble은 줄 기반으로 함수가 중간에 잘림. AST로 함수/클래스 단위 분할. 8개 언어 지원.

**2. 의존성 그래프 (deps/impact)** — 원본 semble에 전혀 없던 기능. import/심볼 추출 → 파일 관계 그래프 → 전이적 영향 분석.

**3. grep 내장 매칭 라인** — 원본 semble은 청크 범위만 반환. 청크 내 키워드 매칭 줄번호 + 코드 라인 추출.

**4. 형제 청크 부스팅** — 긴 파일이 여러 청크로 나뉠 때, 키워드 매칭 청크의 같은 파일 청크도 점수 올림.

**5. 그래프 부스팅** — 검색 상위 파일의 의존/역의존 파일도 점수 부여.

**6. 노이즈 필터** — 1위 점수의 12% 미만 결과 자동 제거. 원본 semble은 항상 k개 전부 반환.

**7. TypeScript export 심볼 추출** — `export function/const/type/interface` 내부까지 파고들어 추출.

**8. Rust mod 선언 + @/ alias 해석** — `mod foo;` → `foo.rs` 매핑, `@/lib/templates` → 실제 파일 매핑.

**9. 유니코드 토크나이저** — 원본 semble은 ASCII(`[a-zA-Z]`)만 토큰화. `\p{L}` 유니코드 문자 클래스로 한글, 일본어, 중국어 등 비 ASCII 텍스트도 BM25 키워드 검색 가능.

## 원본 semble과의 차이 요약

| 항목 | 원본 semble (Python) | semble_rs (Rust) |
| --- | --- | --- |
| 언어 | Python | Rust |
| 설치 | `pip install semble` | `cargo install --path .` |
| 검색 | search, find-related | search, find-related |
| **의존성 분석** | 없음 | **deps, impact** |
| 청킹 | 줄 기반 (고정 길이) | **Tree-sitter AST 기반** |
| 줄번호 | 청크 범위만 | **매칭 줄번호 + 코드 라인** |
| 노이즈 필터 | 없음 (항상 k개 반환) | **점수 기반 자동 필터** |
| 긴 파일 | 누락 발생 | **형제 청크 부스팅** |
| 그래프 부스팅 | 없음 | **의존 파일 검색 반영** |
| 토크나이저 | ASCII만 (`[a-zA-Z]`) | **유니코드 (`\p{L}`) — 한글 등 지원** |
| MCP 서버 | 있음 | 없음 (CLI만) |
| 모델 | potion-code-16M | potion-code-16M (동일) |
| 임베딩 라이브러리 | model2vec + vicinity | ndarray 직접 구현 |
| BM25 라이브러리 | bm25s | 직접 구현 |

## 토큰 절감 비교: semble_rs vs rtk vs grep vs cat

AI 에이전트가 코드를 탐색할 때 소비하는 토큰을 비교합니다.

### 30분 Claude Code 세션 기준

```
┌───────────────────┬──────┬──────────┬──────────┬──────────────────┬──────────┐
│ 작업              │ 횟수 │ cat/read │ rtk      │ semble_rs        │ 절감     │
│                   │      │ (기존)   │ (압축)   │ --compact (대체) │ vs rtk   │
├───────────────────┼──────┼──────────┼──────────┼──────────────────┼──────────┤
│ 코드 검색 (grep)  │  8x  │  16,000  │  3,200   │    4,000         │ 비슷     │
│ 파일 읽기 (cat)   │ 20x  │  40,000  │ 12,000   │        0 (불필요)│ -100%    │
│ 파일 탐색 (ls)    │ 10x  │   2,000  │    400   │        0 (불필요)│ -100%    │
│ 합계              │      │  58,000  │ 15,600   │    4,000         │ -74%     │
└───────────────────┴──────┴──────────┴──────────┴──────────────────┴──────────┘
```

**핵심 차이:**

- **rtk** — 기존 도구(grep, cat, ls)의 출력을 **압축**합니다. 파일은 여전히 읽습니다.
- **semble_rs** — 검색 한 번으로 관련 코드를 찾아서 **읽을 필요 자체를 없앱니다.**

```
rtk 접근:     grep → 출력 압축 → LLM          (읽되 줄인다)
semble_rs:    search --compact → LLM           (안 읽어도 된다)
```

### 실측 비교

JavaScript 프로젝트(80+ 파일)에서 "introPropsHTML" 검색:

```
┌──────────────────────────────┬────────────┬────────────────────────────────┐
│ 방법                         │ 출력 크기  │ 특징                           │
├──────────────────────────────┼────────────┼────────────────────────────────┤
│ grep -rn                     │    296 B   │ 정확한 심볼만 가능             │
│ semble_rs --compact          │    528 B   │ semantic + BM25, 랭킹 포함     │
│ semble_rs --json --strip     │ 21,567 B   │ 주석 제거 + 본문 축약         │
│ semble_rs --json             │ 23,712 B   │ 전체 청크 포함                 │
│ cat (파일 전체 읽기)          │ 48,000 B   │ 관련 없는 코드도 전부 포함     │
└──────────────────────────────┴────────────┴────────────────────────────────┘
```

**--compact은 grep과 비슷한 크기이면서 semantic search가 가능합니다.** 심볼명을 모를 때도 자연어로 찾을 수 있어 cat/read를 완전히 대체합니다.

## 설치

### 1. Rust 툴체인 설치

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### 2. semble_rs 빌드 및 설치

```bash
git clone <repo>
cd semble
cargo install --path .
```

설치 후 `~/.cargo/bin/semble_rs`에 바이너리가 생성됩니다. 첫 실행 시 `potion-code-16M` 모델(\~60MB)이 HuggingFace에서 자동 다운로드됩니다.

### 3. 설치 확인

```bash
semble_rs --help
```

## 검색 팁: 쿼리 작성법

semble_rs는 BM25(키워드) + Semantic(임베딩) 하이브리드 검색입니다. 쿼리 유형에 따라 결과가 크게 달라집니다.

**정확한 심볼을 알 때** — 심볼명을 그대로 사용:

```bash
semble_rs search "introPropsHTML" .           # 0.096점 — 정확히 찾음
semble_rs search "getTitleCardPanelRect" .    # 심볼 쿼리 → BM25 가중치 높임 (alpha=0.3)
```

**정확한 심볼을 모를 때** — 기능을 설명하는 자연어 사용:

```bash
semble_rs search "intro outro panel UI properties" .   # 기능 설명 → 관련 코드 찾음
semble_rs search "aspect ratio export settings" .      # 의미 검색이 보완
```

**하지 마세요** — 심볼명을 추측해서 검색:

```bash
semble_rs search "titleCardPropsHTML" .       # 존재하지 않는 이름 → 0.019점, 부정확
```

존재하지 않는 심볼명은 BM25에서 정확히 매칭되지 않고, 임베딩도 실제 코드와 거리가 멀어 점수가 낮습니다. **모르면 추측하지 말고, 자연어로 기능을 설명하세요.**

### 흔한 실수

```bash
# ❌ 파일 경로를 넘기면 에러 — 프로젝트 디렉토리를 넘겨야 합니다
semble_rs search "query" /path/to/Billing.tsx --json

# ❌ --json은 토큰이 50배 많음 — --compact를 쓰세요
semble_rs search "query" ./my-project --json

# ❌ python으로 파이프 — --compact면 결과가 이미 간결합니다
semble_rs search "query" . --json | python3 -c "..."

# ✅ 올바른 사용
semble_rs search "query" ./my-project --compact
```

## 사용법

```bash
# 코드 검색 — 관련 파일 + 정확한 줄번호 반환 (--compact 권장)
semble_rs search "deletePage" ./my-project --compact
semble_rs search "handleImageUpload" ./my-project --compact

# 의존성 확인 — 심볼, 의존 파일, 사용처
semble_rs deps src/lib/firestore.ts ./my-project

# 영향 분석 — 변경 시 깨지는 파일 전부
semble_rs impact src/lib/firestore.ts ./my-project

# 유사 코드 찾기
semble_rs find-related src/search.rs 91 .

# 토큰 절약 통계
semble_rs savings
```

## Claude Code / Codex Integration

### Option 1: Global CLAUDE.md

Add to `~/.claude/CLAUDE.md` to apply across all projects:

```markdown
# semble_rs — grep, cat, read, ls를 대체하는 코드 검색

`semble_rs` is installed at `~/.cargo/bin/semble_rs`.
ALWAYS use semble_rs instead of grep, cat, read, find.
One search returns file paths + matching lines — no need to read files.

## Rules

1. Code search → `semble_rs search "query" /project/path --compact` (replaces grep/cat/read/ls)
2. Dependencies → `semble_rs deps <file> /project/path --json`
3. Impact analysis → `semble_rs impact <file> /project/path --json`
4. NEVER guess symbol names — use natural language when unsure
5. Only fall back to grep -rn if semble_rs results are insufficient

## Common mistakes

- ALWAYS use `--compact` (not `--json` — 50x more tokens)
- ALWAYS pass a directory path (not a file path — will error)
- NEVER pipe through python — `--compact` output is already concise

## Commands

semble_rs search "query" /path/to/project --compact
semble_rs deps <file> /path/to/project --json
semble_rs impact <file> /path/to/project --json
```

### Option 2: Per-project CLAUDE.md / AGENTS.md

Place `CLAUDE.md` or `AGENTS.md` in the project root for project-specific config:

```bash
cd ./my-project
cat > CLAUDE.md << 'EOF'
# semble_rs — replaces grep, cat, read, ls
semble_rs search "query" . --compact    # code search
semble_rs deps <file> . --json          # dependencies
semble_rs impact <file> . --json        # impact analysis
# ALWAYS use --compact, ALWAYS pass directory path, NEVER guess symbol names
EOF
```

### Verify

Open a new Claude Code session and ask to explore code:

```
❯ Find authentication-related code in this project

⏺ Bash(semble_rs search "authentication" . --compact)
  ⎿ 0.0842  src/lib/firebase.ts:45-89
       L45:  export function loginWithEmail(email, password) {
       L67:  export function signOut() {
```

### Codex

Add the same content to `~/.codex/AGENTS.md` or project root `AGENTS.md`.

## 지원 언어

| 언어 | 검색 | Tree-sitter 청킹 | 의존성 그래프 |
| --- | --- | --- | --- |
| Rust | O | O | O |
| Python | O | O | O |
| JavaScript | O | O | O |
| TypeScript | O | O | O |
| Go | O | O | O |
| Java | O | O | O |
| C | O | O | O |
| C++ | O | O | O |
| Kotlin | O | O | O |
| Ruby, PHP, Swift 등 | O | 줄 기반 fallback | \- |

---

# English README

## semble_rs — AI-Agent-Native Code Search

**`semble_rs` is a fast, accurate code search CLI built for AI coding agents
(Claude Code, Codex, Cursor, Aider, OpenHands, etc.).**
It replaces `grep`, `cat`, `read`, and `ls` with a single hybrid search call
that returns ranked code snippets with exact line numbers — eliminating the
read-file-by-file token waste that dominates real-world agent sessions.

```
Traditional:  ls → grep → cat file1 → cat file2 → ...  (~58,000 tokens / session)
semble_rs:    search --compact                          (~ 4,000 tokens / session, -93%)
```

| Legacy tool | What it does | How semble_rs replaces it |
|---|---|---|
| `grep -rn` | Keyword search over files | `--compact` returns the same size output **plus semantic search** |
| `cat` / `read` | Read a file's contents | Search results include matching lines + line numbers — **no read needed** |
| `ls` / `find` | Discover files / structure | Search results include file paths — **no traversal needed** |

`semble_rs` is a Rust rewrite of the original Python project
[`semble`](https://github.com/MinishLab/semble) by MinishLab,
with a redesigned ranking pipeline and several new capabilities:
**AST-based chunking, dependency graphs, impact analysis, sibling-chunk boosting,
graph boosting, automatic noise filtering, and full Unicode (Korean / CJK)
keyword search.**

### Why agents need this

When an LLM agent answers "how does authentication work in this repo?", it
typically runs many sequential `grep` and `cat` calls, often re-reading the
same files. Each call costs context. `semble_rs` collapses that loop into one
hybrid query that returns ranked, line-numbered snippets so the agent has the
context it needs without ever opening the file.

### What the original `semble` does

MinishLab's `semble` is a Python library for AI-agent code retrieval. Instead
of grepping + reading files one by one, the agent issues a natural-language
query and receives only the relevant code snippets.

**Original pipeline:**

```
query → BM25 (keyword) + Semantic (embedding)
      → RRF hybrid fusion → smart ranking → results
```

**Original characteristics:**

- Python (`model2vec` + `bm25s` + `vicinity`)
- `potion-code-16M` embedding model
- MCP server mode
- Line-based chunking
- ~98% token savings vs. reading whole files

### What `semble_rs` adds / changes

```
Original semble (Python)                    semble_rs (Rust)
┌──────────────────────────┐                ┌──────────────────────────────────┐
│  Search only             │   ──────►      │  Search + deps + impact          │
│  (search, find-related)  │                │  (search, deps, impact, ...)     │
├──────────────────────────┤                ├──────────────────────────────────┤
│  Line-based chunking     │   ──────►      │  Tree-sitter AST chunking        │
│  (functions get cut      │                │  (function / class / struct      │
│   in the middle)         │                │   boundaries respected)          │
├──────────────────────────┤                ├──────────────────────────────────┤
│  No line numbers         │   ──────►      │  Built-in grep — matching line   │
│  (chunk range only)      │                │  numbers + source lines          │
├──────────────────────────┤                ├──────────────────────────────────┤
│  Long files get missed   │   ──────►      │  Sibling-chunk boosting          │
│  (functions in 800+ line │                │  (other chunks of the same       │
│   files often miss top-k)│                │   file get a score lift)         │
├──────────────────────────┤                ├──────────────────────────────────┤
│  No noise filter         │   ──────►      │  Score filter — drops results    │
│  (always returns k)      │                │  scoring below 12% of top hit    │
├──────────────────────────┤                ├──────────────────────────────────┤
│  Python (interpreted)    │   ──────►      │  Rust (compiled)                 │
│  pip install required    │                │  single binary, no runtime deps  │
├──────────────────────────┤                ├──────────────────────────────────┤
│  ASCII-only tokenizer    │   ──────►      │  Unicode tokenizer               │
│  ([a-zA-Z] regex)        │                │  (\p{L} — Korean, CJK, etc.)     │
└──────────────────────────┘                └──────────────────────────────────┘
```

### How it works — the search pipeline

```
                  ┌─────────────────────────────────────────────────────────────┐
                  │                Indexing (per run)                           │
                  │                                                             │
                  │  Tree-sitter AST     Embedding encoder    Dependency graph  │
                  │  (function/class    (potion-code-16M)     (imports +        │
                  │   chunking)          → safetensors)        symbols)         │
                  │        │                  │                    │            │
                  │        ▼                  ▼                    ▼            │
                  │   BM25 index         Vector index         File graph        │
                  └─────────────────────────────────────────────────────────────┘

                  ┌─────────────────────────────────────────────────────────────┐
                  │                Search (per query)                           │
                  │                                                             │
                  │      BM25 score          Semantic score                     │
                  │            \                /                               │
                  │             ▼              ▼                                │
                  │         RRF hybrid fusion (alpha-weighted)                  │
                  │                       │                                     │
                  │                       ▼                                     │
                  │     Smart ranking:                                          │
                  │       · definition boost                                    │
                  │       · file-consistency boost                              │
                  │       · sibling-chunk boost   ◄── fixes long-file misses    │
                  │       · graph boost           ◄── follows dependencies      │
                  │       · test-file penalty                                   │
                  │                       │                                     │
                  │                       ▼                                     │
                  │     Noise filter (drop < 12% of top score)                  │
                  │                       │                                     │
                  │                       ▼                                     │
                  │     Matching-line extraction (built-in grep)                │
                  └─────────────────────────────────────────────────────────────┘
                                          │
                                          ▼
                                  ## 1. src/lib/auth.ts
                                    L45: function login()
                                    L89: auth.signIn()

                                  ## 2. src/api/route.ts
                                    L12: import { login }
```

### Dependency & impact analysis (new — not in upstream)

```
semble_rs deps src/lib/firestore.ts .

    firestore.ts
    ├── defines:   upsertUser, getUser, createPage, deletePage, ...
    ├── imports:   firebase.ts
    └── used by:   [slug]/page.tsx, api/pages/route.ts, dashboard/page.tsx, ...

semble_rs impact src/lib/firestore.ts .

    Changing firestore.ts affects:
    ├── direct importers:  api/pages/route.ts, dashboard/page.tsx, ...
    └── transitive impact: api/admin/route.ts (dashboard → admin)
```

`deps` and `impact` make `semble_rs` useful for change-safety review and refactor
planning — workflows the original `semble` (Python) does not cover.

### Why Rust?

```
Python runtime                             Rust binary
┌──────────────────────────┐               ┌──────────────────────────────┐
│  pip install required    │               │  Single binary (~15 MB)      │
│  Python 3.10+ required   │   ──────►     │  No runtime dependencies     │
│  venv recommended        │               │  Runs anywhere               │
├──────────────────────────┤               ├──────────────────────────────┤
│  model2vec (PyTorch)     │               │  ndarray + safetensors       │
│  bm25s (NumPy)           │   ──────►     │  Pure Rust implementation    │
│  vicinity (NumPy)        │               │  No external ML stack        │
│  ~200 MB+ of deps        │               │                              │
├──────────────────────────┤               ├──────────────────────────────┤
│  GIL → single-threaded   │               │  Native multithreading       │
│  Interpreter overhead    │   ──────►     │  Compile-time optimization   │
│                          │               │  Zero-copy string handling   │
└──────────────────────────┘               └──────────────────────────────┘
```

For an agent tool that runs thousands of times across many projects, **installation
simplicity and execution speed matter more than language ergonomics.** A single
binary that runs anywhere with no `pip` ceremony is Rust's biggest win here.

### What is preserved from the original `semble` — and what is new

#### Preserved (faithful port from `semble` Python)

| Concept | Original (Python) | semble_rs (Rust) |
| --- | --- | --- |
| BM25 index | `bm25s` library | `bm25.rs` — direct implementation |
| Semantic embedding | `model2vec.StaticModel` | `encoder.rs` — loads `safetensors` directly |
| Vector search | `vicinity.CosineBasicBackend` | `encoder.rs` — `ndarray` dot product |
| RRF hybrid fusion | `search.py` | `search.rs` — same `1/(k+rank)` formula |
| Alpha weighting | `weighting.py` | `ranking/weighting.rs` — symbol 0.3, NL 0.5 |
| Definition boost | `boosting.py` | `ranking/boosting.rs` — same regex |
| Test-file penalty | `penalties.py` | `ranking/penalties.rs` — same patterns |
| Tokenizer | `tokens.py` | `tokens.rs` — camelCase/snake_case split + **Unicode** |
| File walker | `file_walker.py` | `file_walker.rs` — `.gitignore` aware |

#### New (not in the original `semble`)

1. **Tree-sitter AST chunking** — the original `semble` splits files by line
   count, so functions get cut in half. `semble_rs` chunks by AST node (function,
   class, struct, etc.). 8 languages supported with AST; others fall back to
   line-based.
2. **Dependency graph (`deps` / `impact`)** — entirely new, not present in the
   original `semble`. Extracts imports and exported symbols, builds an
   inter-file graph, and computes transitive impact.
3. **Built-in grep in results** — the original `semble` only returns chunk
   ranges. `semble_rs` extracts matching line numbers + source lines inside
   each chunk.
4. **Sibling-chunk boosting** — for long files split across multiple chunks,
   if one chunk matches the query, sibling chunks get a score lift. This fixes
   real misses where the actual definition is in a different chunk than the
   call site.
5. **Graph boosting** — files that depend on (or are depended on by) top-ranked
   results get a boost. Helps when the relevant code is "one hop away."
6. **Noise filter** — drops results scoring below 12% of the top hit. The
   original `semble` always returns exactly `k`, padding the agent's context with junk.
7. **TypeScript export-symbol extraction** — descends into `export function`,
   `export const`, `export type`, and `export interface` declarations.
8. **Rust `mod` + `@/` alias resolution** — maps `mod foo;` → `foo.rs` and
   path aliases like `@/lib/templates` to real files for accurate dep edges.
9. **Unicode tokenizer** — the original `semble` uses `[a-zA-Z]` and silently
   drops non-ASCII tokens. `semble_rs` uses `\p{L}`, so Korean / Japanese /
   Chinese identifiers, comments, and docs are all indexed and searchable via
   BM25.

### Diff vs. upstream — quick reference

| Feature | Upstream `semble` (Python) | `semble_rs` (Rust) |
| --- | --- | --- |
| Language | Python | Rust |
| Install | `pip install semble` | `cargo install --path .` |
| Search | `search`, `find-related` | `search`, `find-related` |
| **Dependency analysis** | — | **`deps`, `impact`** |
| Chunking | Line-based (fixed length) | **Tree-sitter AST** |
| Line numbers | Chunk range only | **Match line numbers + source lines** |
| Noise filter | None (always `k` results) | **Score-based auto filter** |
| Long files | Often missed | **Sibling-chunk boosting** |
| Graph boost | — | **Dependency-aware ranking** |
| Tokenizer | ASCII only (`[a-zA-Z]`) | **Unicode (`\p{L}`) — Korean etc.** |
| MCP server | Yes | No (CLI only) |
| Embedding model | `potion-code-16M` | `potion-code-16M` (same) |
| Embedding lib | `model2vec` + `vicinity` | `ndarray` (direct) |
| BM25 lib | `bm25s` | Direct implementation |

### Token savings — `semble_rs` vs. `rtk` vs. `grep` vs. `cat`

Tokens consumed by an agent while exploring code.

#### 30-minute Claude Code session

```
┌───────────────────┬──────┬──────────┬──────────┬──────────────────┬──────────┐
│ Operation         │ Calls│ cat/read │ rtk      │ semble_rs        │ Savings  │
│                   │      │ (legacy) │ (compress│ --compact (replace) │ vs rtk│
├───────────────────┼──────┼──────────┼──────────┼──────────────────┼──────────┤
│ Code search (grep)│  8x  │  16,000  │   3,200  │     4,000        │   ~same  │
│ File read (cat)   │ 20x  │  40,000  │  12,000  │         0        │  -100%   │
│ File listing (ls) │ 10x  │   2,000  │     400  │         0        │  -100%   │
│ Total             │      │  58,000  │  15,600  │     4,000        │  -74%    │
└───────────────────┴──────┴──────────┴──────────┴──────────────────┴──────────┘
```

**Key distinction:**

- **`rtk`** compresses the output of legacy tools (`grep`, `cat`, `ls`). You
  still read the files.
- **`semble_rs`** returns the right snippets in one search, so the file read
  never happens.

```
rtk approach:    grep → compress output → LLM       (read but smaller)
semble_rs:       search --compact → LLM             (don't read at all)
```

#### Measured comparison

JavaScript project (80+ files), query `introPropsHTML`:

```
┌──────────────────────────────┬────────────┬────────────────────────────────┐
│ Method                       │ Output     │ Notes                          │
├──────────────────────────────┼────────────┼────────────────────────────────┤
│ grep -rn                     │    296 B   │ Exact symbol only              │
│ semble_rs --compact          │    528 B   │ Semantic + BM25 with ranking   │
│ semble_rs --json --strip     │ 21,567 B   │ Comments stripped, body trimmed│
│ semble_rs --json             │ 23,712 B   │ Full chunks                    │
│ cat (full file)              │ 48,000 B   │ Includes unrelated code        │
└──────────────────────────────┴────────────┴────────────────────────────────┘
```

`--compact` is roughly the same size as `grep -rn` **but adds semantic search
and ranking** — so it works even when the agent doesn't know the exact symbol
name. That makes it a viable replacement for `cat` / `read`, not just for `grep`.

### Installation

#### 1. Install the Rust toolchain

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

#### 2. Build and install `semble_rs`

```bash
git clone https://github.com/johunsang/semble_rs.git
cd semble_rs
cargo install --path .
```

The binary is installed to `~/.cargo/bin/semble_rs`. On first run the
`potion-code-16M` model (~60 MB) is downloaded from HuggingFace automatically.

#### 3. Verify

```bash
semble_rs --help
```

### Query writing — getting good results

`semble_rs` is a hybrid BM25 + semantic search. The query style strongly affects
the result quality:

**When you know the exact symbol** — use the symbol name as-is:

```bash
semble_rs search "introPropsHTML" .          # 0.096 — exact match
semble_rs search "getTitleCardPanelRect" .   # symbol query → BM25-heavy (alpha=0.3)
```

**When you do not know the symbol** — describe the feature in natural language:

```bash
semble_rs search "intro outro panel UI properties" .   # feature description
semble_rs search "aspect ratio export settings" .      # semantic helps here
```

**Avoid** — guessing a symbol name:

```bash
semble_rs search "titleCardPropsHTML" .      # nonexistent name → low score, noise
```

A guessed symbol fails BM25 (no exact match) **and** drifts in embedding space,
so the score is low. **If you don't know the name, describe the behavior — don't
invent the name.**

#### Common mistakes

```bash
# ❌ Passing a file path — the path argument must be a directory
semble_rs search "query" /path/to/Billing.tsx --json

# ❌ --json is ~50x more tokens — use --compact
semble_rs search "query" ./my-project --json

# ❌ Piping through python — --compact is already concise
semble_rs search "query" . --json | python3 -c "..."

# ✅ Correct usage
semble_rs search "query" ./my-project --compact
```

### Usage

```bash
# Code search — returns relevant files + exact line numbers (use --compact)
semble_rs search "deletePage" ./my-project --compact
semble_rs search "handleImageUpload" ./my-project --compact

# Dependencies — defined symbols, imports, callers
semble_rs deps src/lib/firestore.ts ./my-project

# Impact analysis — files broken by a change to this file
semble_rs impact src/lib/firestore.ts ./my-project

# Find code similar to a specific chunk
semble_rs find-related src/search.rs 91 .

# Cumulative token-savings stats
semble_rs savings
```

### Integration with Claude Code, Codex, Cursor, Aider, OpenHands

#### Option 1 — Global `CLAUDE.md`

Add this to `~/.claude/CLAUDE.md` to apply across all projects:

```markdown
# semble_rs — replaces grep, cat, read, ls

`semble_rs` is installed at `~/.cargo/bin/semble_rs`.
ALWAYS use semble_rs instead of grep, cat, read, find.
One search returns file paths + matching lines — no need to read files.

## Rules

1. Code search → `semble_rs search "query" /project/path --compact`
2. Dependencies → `semble_rs deps <file> /project/path --json`
3. Impact analysis → `semble_rs impact <file> /project/path --json`
4. NEVER guess symbol names — use natural language when unsure
5. Fall back to `grep -rn` only when semble_rs results are insufficient

## Common mistakes

- ALWAYS use `--compact` (not `--json` — 50x more tokens)
- ALWAYS pass a directory path (not a file path — will error)
- NEVER pipe through python — `--compact` output is already concise
```

#### Option 2 — Per-project `CLAUDE.md` / `AGENTS.md`

Drop a minimal instruction file in the project root:

```bash
cd ./my-project
cat > CLAUDE.md << 'EOF'
# semble_rs — replaces grep, cat, read, ls
semble_rs search "query" . --compact    # code search
semble_rs deps <file> . --json          # dependencies
semble_rs impact <file> . --json        # impact analysis
# ALWAYS use --compact, ALWAYS pass a directory path, NEVER guess symbol names
EOF
```

#### Verify

Open a new agent session and ask it to explore the code:

```
❯ Find authentication-related code in this project

⏺ Bash(semble_rs search "authentication" . --compact)
  ⎿ 0.0842  src/lib/firebase.ts:45-89
       L45:  export function loginWithEmail(email, password) {
       L67:  export function signOut() {
```

#### Codex / Cursor / Aider / OpenHands

The same `CLAUDE.md` / `AGENTS.md` content works for Codex (`~/.codex/AGENTS.md`),
Cursor (`.cursorrules`), Aider (`.aider.conf.yml` → `read`), and OpenHands
(`.openhands_instructions`). Any agent that supports project-level instructions
can be steered to use `semble_rs` first.

### Supported languages

| Language | Search | Tree-sitter chunking | Dependency graph |
| --- | --- | --- | --- |
| Rust | ✓ | ✓ | ✓ |
| Python | ✓ | ✓ | ✓ |
| JavaScript | ✓ | ✓ | ✓ |
| TypeScript | ✓ | ✓ | ✓ |
| Go | ✓ | ✓ | ✓ |
| Java | ✓ | ✓ | ✓ |
| C | ✓ | ✓ | ✓ |
| C++ | ✓ | ✓ | ✓ |
| Kotlin | ✓ | ✓ | ✓ |
| Ruby, PHP, Swift, ... | ✓ | line-based fallback | — |

### FAQ

**Q. Does this run an LLM?**
No. `semble_rs` is a deterministic search tool. It uses a small static embedding
model (`potion-code-16M`, ~60 MB) for semantic similarity — no LLM, no network
calls after the first model download.

**Q. Is there an index file?**
The index is built per run from the current state of the directory. No stale
cache to invalidate, no background daemon. Indexing a medium project takes a
few seconds.

**Q. Does it respect `.gitignore`?**
Yes. The file walker honors `.gitignore` and standard ignore rules out of the
box.

**Q. Why not call it `semble`?**
Out of respect for the upstream project and to avoid a name collision on
crates.io. The CLI binary is `semble_rs`; the crate is `semble`.

**Q. MCP server?**
Not yet. The upstream Python project has one; `semble_rs` is CLI-only for now.
Most agent harnesses run shell commands well, so the CLI form already covers
the main use cases.

### Credits

- Upstream: [MinishLab/semble](https://github.com/MinishLab/semble) —
  the original `semble` (Python) implementation and the ranking ideas that this
  project faithfully ports.
- Embedding model: [`minishlab/potion-code-M`](https://huggingface.co/minishlab/potion-base-32M)
  family by MinishLab.
- Built on: `tree-sitter`, `ndarray`, `safetensors`, `tokenizers`, `hf-hub`,
  `ignore`, `clap`.

### License

MIT

---

## 라이선스

MIT
