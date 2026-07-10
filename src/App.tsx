import { useEffect, useState, useRef } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import './App.css'

type ReqField = {
  id: string
  key: string
  value: string
}

type CutRequirement = {
  fields: ReqField[]
  targetDuration: number
  tolerance: number
}

type CutTemplate = {
  id: string
  name: string
  requirement: CutRequirement
  builtin: boolean
  updatedAt: string
}

type TemplateList = {
  templates: CutTemplate[]
}

type AIConfig = {
  baseUrl: string
  apiKey: string
  model: string
  fps: number
  frameMaxSize: number
  useAi: boolean
  groupByVideo: boolean
  trimLastShortSegment: boolean
  requirement: CutRequirement
  currentTemplateId: string | null
  lastInputFolder: string
  lastOutputFolder: string
  usedVideoAction: 'none' | 'delete' | 'move'
}

type VideoFile = {
  path: string
  name: string
  size: number
  duration: number | null
}

type CutSegment = {
  output_path: string
  source: string
  src_start: number
  src_end: number
  duration: number
  reason: string | null
}

type CutResult = {
  source_name: string
  segments: CutSegment[]
  error: string | null
}

const newField = (key: string, value: string): ReqField => ({
  id: `f_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
  key,
  value,
})

const DEFAULT_FIELDS: ReqField[] = [
  newField('素材描述', '随手拍/混剪通用素材'),
  newField('用途', '用于二次剪辑的插入素材'),
  newField('想要的镜头', '主体清晰、构图好、有明确动作'),
  newField('不要的镜头', '黑屏/模糊/对焦失败/剧烈抖动/静止'),
]

const DEFAULT_CONFIG: AIConfig = {
  // 默认全部留空，避免把开发用的 API key / 中转站地址硬编码进 bundle
  // 用户首次打开需要在「AI 设置」里填入自己的 baseUrl / apiKey / model
  baseUrl: '',
  apiKey: '',
  model: '',
  fps: 1,
  frameMaxSize: 480,
  useAi: false,  // 默认纯裁切
  groupByVideo: false,
  trimLastShortSegment: true,  // 纯裁切：最后一段时长不足目标时长就不保存（默认开启）
  requirement: {
    fields: DEFAULT_FIELDS,
    targetDuration: 5,
    tolerance: 1,
  },
  currentTemplateId: null,
  lastInputFolder: '',
  lastOutputFolder: '',
  usedVideoAction: 'none',
}

const DURATION_PRESETS = [3, 5, 8, 10, 15, 20, 30]
const TOLERANCE_PRESETS = [0, 0.5, 1, 2, 3]

export default function App() {
  const [inputFolder, setInputFolder] = useState('')
  const [outputFolder, setOutputFolder] = useState('')
  const [videos, setVideos] = useState<VideoFile[]>([])
  const [config, setConfig] = useState<AIConfig>(DEFAULT_CONFIG)
  const configRef = useRef<AIConfig>(config)
  configRef.current = config  // 始终指向最新 state
  const [customDuration, setCustomDuration] = useState('')
  const [customTolerance, setCustomTolerance] = useState('')
  const [showSettings, setShowSettings] = useState(false)
  const [testStatus, setTestStatus] = useState<string>('')
  const [running, setRunning] = useState(false)
  const [progress, setProgress] = useState<{ current: number; total: number; item: string; stage: string; [k: string]: any }>({ current: 0, total: 0, item: '', stage: '' })
  const [results, setResults] = useState<CutResult[]>([])
  const [logs, setLogs] = useState<string[]>([])
  const logBoxRef = useRef<HTMLDivElement>(null)

  // 模板管理
  const [templateList, setTemplateList] = useState<TemplateList>({ templates: [] })
  const [editingTemplateName, setEditingTemplateName] = useState<string>('')

  useEffect(() => {
    if (logBoxRef.current) {
      logBoxRef.current.scrollTop = logBoxRef.current.scrollHeight
    }
  }, [logs])

  // 初次加载
  useEffect(() => {
    invoke<AIConfig>('load_ai_config').then((c) => {
      const req = c.requirement || DEFAULT_CONFIG.requirement
      const merged = {
        ...DEFAULT_CONFIG,
        ...c,
        requirement: {
          fields: req.fields && req.fields.length > 0 ? req.fields : DEFAULT_FIELDS,
          targetDuration: req.targetDuration ?? 5,
          tolerance: req.tolerance ?? 1,
        },
        currentTemplateId: c.currentTemplateId ?? null,
      }
      setConfig(merged)
      const tpl = (req.fields && req.fields.length > 0 ? req : DEFAULT_CONFIG.requirement)
      setEditingTemplateName(tpl.fields?.[0]?.value?.slice(0, 8) ?? '未命名')

      // 恢复上次选择的输入/输出文件夹
      if (c.lastInputFolder) {
        setInputFolder(c.lastInputFolder)
        invoke<VideoFile[]>('scan_folder', { folder: c.lastInputFolder })
          .then(setVideos)
          .catch(() => {})
      }
      if (c.lastOutputFolder) {
        setOutputFolder(c.lastOutputFolder)
      }
    }).catch(() => {})

    invoke<TemplateList>('list_templates').then(setTemplateList).catch(() => {})
  }, [])

  // 同步模板列表
  const refreshTemplates = async () => {
    try {
      const list = await invoke<TemplateList>('list_templates')
      setTemplateList(list)
    } catch {}
  }

  useEffect(() => {
    const u1 = listen<{ current: number; total: number; item: string; stage?: string; [k: string]: any }>(
      'cut-progress',
      (e) => {
        setProgress({ ...e.payload, stage: e.payload.stage ?? '' })
        const stage = e.payload.stage ?? ''
        const stageLabel =
          stage === 'probe' ? '探测时长'
          : stage === 'extract_frames' ? '抽帧'
          : stage === 'loading_frames' ? '加载帧'
          : stage === 'pure_start' ? '纯裁剪'
          : stage === 'pure_cutting' ? '纯裁切中'
          : stage === 'ai_analyzing' ? 'AI 分析'
          : stage === 'ai_done' ? 'AI 完成'
          : stage === 'cutting' ? '切段'
          : stage === 'pure_cut' ? '纯裁剪'
          : stage === 'done' ? '完成'
          : stage === 'start' ? '开始'
          : '处理'
        let extra = ''
        if (stage === 'ai_done') extra = `（${e.payload.valid_shots ?? 0} 段）`
        if (stage === 'done') extra = `（${e.payload.segments ?? 0} 段）`
        if (stage === 'ai_analyzing') extra = `（${e.payload.frames ?? 0} 帧）`
        setLogs((prev) => [
          ...prev.slice(-300),
          `[${stageLabel}] ${e.payload.current}/${e.payload.total} ${e.payload.item}${extra}`,
        ])
      },
    )
    const u2 = listen('cut-done', () => {
      setRunning(false)
      setLogs((prev) => [...prev.slice(-300), '✓ 全部完成'])
    })
    return () => {
      u1.then((f) => f())
      u2.then((f) => f())
    }
  }, [])

  const pickInput = async () => {
    const s = await open({ directory: true, multiple: false })
    if (typeof s !== 'string') return
    setInputFolder(s)
    // 持久化：记住上次选的输入文件夹
    const next = { ...configRef.current, lastInputFolder: s }
    setConfig(next)
    configRef.current = next
    saveConfig(next)
    try {
      const list: VideoFile[] = await invoke('scan_folder', { folder: s })
      setVideos(list)
    } catch (e: any) {
      alert(`扫描失败: ${e}`)
    }
  }

  const pickOutput = async () => {
    const s = await open({ directory: true, multiple: false })
    if (typeof s !== 'string') return
    setOutputFolder(s)
    // 持久化：记住上次选的输出文件夹
    const next = { ...configRef.current, lastOutputFolder: s }
    setConfig(next)
    configRef.current = next
    saveConfig(next)
  }

  const saveConfig = async (cfg: AIConfig) => {
    try {
      await invoke('save_ai_config', { config: cfg })
    } catch (e: any) {
      alert(`保存失败: ${e}`)
    }
  }

  const testAI = async () => {
    setTestStatus('测试中…')
    try {
      const r: string = await invoke('test_connection', { cfg: config })
      setTestStatus(r)
    } catch (e: any) {
      setTestStatus(`✗ ${e}`)
    }
  }

  // ====== 字段操作 ======
  const updateField = (id: string, patch: Partial<ReqField>) => {
    setConfig((c) => ({
      ...c,
      requirement: {
        ...c.requirement,
        fields: c.requirement.fields.map((f) => (f.id === id ? { ...f, ...patch } : f)),
      },
      currentTemplateId: c.currentTemplateId ? null : c.currentTemplateId, // 编辑后断开模板
    }))
  }

  const addField = () => {
    setConfig((c) => ({
      ...c,
      requirement: {
        ...c.requirement,
        fields: [...c.requirement.fields, newField('新字段', '')],
      },
      currentTemplateId: c.currentTemplateId ? null : c.currentTemplateId,
    }))
  }

  const removeField = (id: string) => {
    setConfig((c) => ({
      ...c,
      requirement: {
        ...c.requirement,
        fields: c.requirement.fields.filter((f) => f.id !== id),
      },
      currentTemplateId: c.currentTemplateId ? null : c.currentTemplateId,
    }))
  }

  // ====== 模板操作 ======
  const loadTemplate = (tpl: CutTemplate) => {
    // 防御性去重：templates.json 里偶尔会有重复 id（uuid_simple 早期版本碰撞），
    // React 渲染遇到重复 key 会抛错并 unmount 整个组件树
    const seen = new Set<string>()
    const fields = tpl.requirement.fields.map((f, i) => {
      if (seen.has(f.id)) {
        return { ...f, id: `f_dedupe_${Date.now()}_${i}_${Math.random().toString(36).slice(2, 6)}` }
      }
      seen.add(f.id)
      return f
    })
    setConfig((c) => ({
      ...c,
      requirement: { ...tpl.requirement, fields },
      currentTemplateId: tpl.id,
    }))
    setEditingTemplateName(tpl.name)
  }

  const newTemplate = () => {
    setConfig((c) => ({
      ...c,
      requirement: {
        fields: [newField('新模板', '描述你的素材...')],
        targetDuration: 5,
        tolerance: 1,
      },
      currentTemplateId: null,
    }))
    setEditingTemplateName('新模板')
  }

  const saveAsNew = async () => {
    const name = (editingTemplateName || '').trim() || `模板-${Date.now()}`
    try {
      const t = await invoke<CutTemplate>('save_template', {
        name,
        requirement: config.requirement,
        existingId: null,
      })
      await refreshTemplates()
      setConfig((c) => ({ ...c, currentTemplateId: t.id }))
      alert(`已保存: ${t.name}`)
    } catch (e: any) {
      alert(`保存失败: ${e}`)
    }
  }

  const overwriteCurrent = async () => {
    if (!config.currentTemplateId) {
      alert('当前未载入模板，请用"保存为新"')
      return
    }
    const tpl = templateList.templates.find((t) => t.id === config.currentTemplateId)
    if (!tpl) { alert('模板不存在'); return }
    if (tpl.builtin) { alert('内置模板不能覆盖，请用"保存为新"'); return }
    try {
      await invoke('save_template', {
        name: tpl.name,
        requirement: config.requirement,
        existingId: tpl.id,
      })
      await refreshTemplates()
      alert(`已覆盖: ${tpl.name}`)
    } catch (e: any) {
      alert(`覆盖失败: ${e}`)
    }
  }

  const deleteTpl = async (id: string) => {
    if (!confirm('确定删除该模板？')) return
    try {
      await invoke('delete_template', { id })
      await refreshTemplates()
      if (config.currentTemplateId === id) {
        setConfig((c) => ({ ...c, currentTemplateId: null }))
      }
    } catch (e: any) {
      alert(`删除失败: ${e}`)
    }
  }

  const effectiveDuration = customDuration
    ? parseFloat(customDuration) || config.requirement.targetDuration
    : config.requirement.targetDuration
  const effectiveTolerance = customTolerance
    ? parseFloat(customTolerance) || config.requirement.tolerance
    : config.requirement.tolerance

  const canRun = !!inputFolder && videos.length > 0 && !!outputFolder && !running

  // 切完收集「被处理过的视频」完整路径，根据 usedVideoAction 调用后端
  const processUsedVideos = async (res: CutResult[]) => {
    const action = config.usedVideoAction
    if (action === 'none') return
    // 只处理成功（没 error）且确实切出段的视频，避免误删没成功的
    const usedNames = new Set<string>()
    for (const r of res) {
      if (r.error) continue
      if (r.segments.length === 0) continue
      usedNames.add(r.source_name)
    }
    if (usedNames.size === 0) {
      setLogs(prev => [...prev, '用过的素材处理：本次没有被成功切段的视频，跳过'])
      return
    }
    const paths: string[] = []
    for (const v of videos) {
      if (usedNames.has(v.name)) paths.push(v.path)
    }
    try {
      const msg = await invoke<string>('handle_used_videos', {
        action,
        paths,
        inputFolder,
      })
      setLogs(prev => [...prev, `用过的素材处理(${action})：${msg}`])
      // 重新扫描输入目录刷新列表（移动/删除后视图同步）
      if (action === 'delete' || action === 'move') {
        const list: VideoFile[] = await invoke('scan_folder', { folder: inputFolder })
        setVideos(list)
      }
    } catch (e: any) {
      setLogs(prev => [...prev, `用过的素材处理失败: ${e}`])
      alert(`用过的素材处理失败: ${e}`)
    }
  }

  // 纯裁切 - 调用独立命令，绝不走 AI
  const runPureCut = async () => {
    if (!canRun) return
    setRunning(true)
    setResults([])
    setLogs([])
    console.log('[可乐裁切] 调用 run_pure_cut 纯裁切')
    try {
      const res: CutResult[] = await invoke('run_pure_cut', {
        inputFolder,
        outputFolder,
        targetDuration: effectiveDuration,
        groupByVideo: config.groupByVideo,
        trimLastShortSegment: config.trimLastShortSegment,
      })
      setResults(res)
      await processUsedVideos(res)
    } catch (e: any) {
      alert(`纯裁切失败: ${e}`)
    } finally {
      setRunning(false)
    }
  }

  // AI 裁切 - 调用独立命令
  const runAiCut = async () => {
    if (!canRun) return
    setRunning(true)
    setResults([])
    setLogs([])
    console.log('[可乐裁切] 调用 run_ai_cut AI 裁切')
    const finalConfig: AIConfig = {
      ...config,
      useAi: true,
      requirement: {
        ...config.requirement,
        targetDuration: effectiveDuration,
        tolerance: effectiveTolerance,
      },
    }
    try {
      await saveConfig(finalConfig)
      const res: CutResult[] = await invoke('run_ai_cut', {
        inputFolder,
        outputFolder,
        config: finalConfig,
      })
      setResults(res)
      await processUsedVideos(res)
    } catch (e: any) {
      alert(`AI 裁切失败: ${e}`)
    } finally {
      setRunning(false)
    }
  }

  return (
    <div className="app">
      <header className="header">
        <h1>可乐裁切</h1>
        <span className="ver">v0.66</span>
      </header>

      <main className="main">
        {/* 左：配置 */}
        <section className="panel">
          <h2>输入素材</h2>
          <div className="row" style={{ marginBottom: 8 }}>
            <button onClick={pickInput}>选择文件夹</button>
            <span className="path">{inputFolder || '未选择'}</span>
          </div>
          <div className="meta">{videos.length} 个视频</div>

          <div style={{ marginTop: 10, fontSize: 12 }}>
            <span style={{ color: '#888', marginRight: 8 }}>用过的素材处理：</span>
            <select
              value={config.usedVideoAction}
              onChange={e => {
                const v = e.target.value as AIConfig['usedVideoAction']
                setConfig({ ...config, usedVideoAction: v })
              }}
              style={{
                padding: '4px 8px', background: '#1e1e1e', color: '#fff',
                border: '1px solid #444', borderRadius: 4, fontSize: 12,
              }}
            >
              <option value="none">不处理</option>
              <option value="delete">删除</option>
              <option value="move">放进子文件夹</option>
            </select>
            <div style={{ fontSize: 11, color: '#666', marginTop: 4 }}>
              {config.usedVideoAction === 'move'
                ? '会在输入文件夹下新建 used/ 子文件夹，把用过的素材移进去'
                : config.usedVideoAction === 'delete'
                ? '⚠ 用过的素材会被直接删除，删了就没了'
                : '用过的素材留在原位不动'}
            </div>
          </div>

          <h2 style={{ marginTop: 24 }}>输出</h2>
          <div className="row" style={{ marginBottom: 8 }}>
            <button onClick={pickOutput}>选择输出文件夹</button>
            <span className="path">{outputFolder || '未选择'}</span>
          </div>
          <div style={{ marginTop: 8, fontSize: 12, color: '#888' }}>
            组织方式：
            <label style={{ marginLeft: 8, cursor: 'pointer' }}>
              <input
                type="radio"
                checked={!config.groupByVideo}
                onChange={() => setConfig({ ...config, groupByVideo: false })}
              /> 平铺输出
            </label>
            <label style={{ marginLeft: 12, cursor: 'pointer' }}>
              <input
                type="radio"
                checked={config.groupByVideo}
                onChange={() => setConfig({ ...config, groupByVideo: true })}
              /> 按视频分组
            </label>
          </div>
          <div className="meta" style={{ fontSize: 11, color: '#666', marginTop: 4 }}>
            {config.groupByVideo
              ? '<视频名>/seg_NNN.mp4'
              : '<视频名>_seg_NNN.mp4（全部平铺到选定文件夹）'}
          </div>

          {/* 模式选择 - 直接传硬编码值，不依赖 state */}
          <h2 style={{ marginTop: 24 }}>裁切模式</h2>
          <div style={{ display: 'flex', gap: 8, marginBottom: 12 }}>
            <button
              onClick={() => setConfig({ ...config, useAi: false })}
              style={{
                flex: 1, padding: '10px',
                background: !config.useAi ? '#3b82f6' : '#2a2a2a',
                color: '#fff', border: '1px solid #444', borderRadius: 6,
                cursor: 'pointer', fontWeight: !config.useAi ? 'bold' : 'normal', fontSize: 13,
              }}
            >
              ✂ 纯裁切
              <div style={{ fontSize: 10, opacity: 0.7, marginTop: 2 }}>按固定时长等分</div>
            </button>
            <button
              onClick={() => setConfig({ ...config, useAi: true })}
              style={{
                flex: 1, padding: '10px',
                background: config.useAi ? '#8b5cf6' : '#2a2a2a',
                color: '#fff', border: '1px solid #444', borderRadius: 6,
                cursor: 'pointer', fontWeight: config.useAi ? 'bold' : 'normal', fontSize: 13,
              }}
            >
              🤖 AI 裁切
              <div style={{ fontSize: 10, opacity: 0.7, marginTop: 2 }}>AI 智能挑选片段</div>
            </button>
          </div>
          {config.useAi && (
            <button
              onClick={() => setShowSettings(true)}
              style={{
                width: '100%', padding: '6px', marginBottom: 12,
                background: '#2a2a2a', border: '1px solid #444', borderRadius: 4,
                color: '#aaa', cursor: 'pointer', fontSize: 11,
              }}
            >
              ⚙ AI 设置（API / 模板）
            </button>
          )}

          <h2>裁切参数</h2>

          <label>每段时长（秒）</label>
          <div style={{ display: 'flex', gap: 6, marginTop: 4, flexWrap: 'wrap' }}>
            {DURATION_PRESETS.map((t) => (
              <button
                key={t}
                onClick={() => {
                  setConfig({ ...config, requirement: { ...config.requirement, targetDuration: t } })
                  setCustomDuration('')
                }}
                style={{
                  background: !customDuration && t === config.requirement.targetDuration ? '#3b82f6' : '#2a2a2a',
                  color: '#fff',
                  padding: '6px 14px',
                  border: '1px solid #444',
                  borderRadius: 4,
                }}
              >
                {t}s
              </button>
            ))}
          </div>
          <div style={{ marginTop: 8, display: 'flex', alignItems: 'center', gap: 6 }}>
            <span style={{ fontSize: 12, color: '#888' }}>自定义:</span>
            <input
              type="number"
              min={0.5}
              max={60}
              step={0.5}
              value={customDuration}
              onChange={(e) => setCustomDuration(e.target.value)}
              placeholder={String(config.requirement.targetDuration)}
              style={{ width: 80, padding: '4px 8px' }}
            />
            <span style={{ fontSize: 12, color: '#888' }}>秒</span>
            <span style={{ fontSize: 12, color: '#34d399', marginLeft: 8 }}>
              当前: {effectiveDuration.toFixed(1)}s
            </span>
          </div>

          {!config.useAi && (
            <label style={{
              marginTop: 12, display: 'flex', alignItems: 'center', gap: 8,
              cursor: 'pointer', fontSize: 13, padding: 8, background: '#2a2a2a', borderRadius: 4,
            }}>
              <input
                type="checkbox"
                checked={config.trimLastShortSegment}
                onChange={(e) => setConfig({ ...config, trimLastShortSegment: e.target.checked })}
                style={{ width: 16, height: 16 }}
              />
              <span>
                <strong>去尾段</strong>：最后一段时长不足 {effectiveDuration.toFixed(1)}s 就跳过不保存
                <span style={{ fontSize: 11, color: '#888', display: 'block', marginTop: 2 }}>
                  关闭则最后一段会被裁到视频末尾（时长会小于目标）
                </span>
              </span>
            </label>
          )}

          {config.useAi && (
            <>
              <label style={{ marginTop: 12, display: 'block' }}>允许偏差（秒）</label>
              <div style={{ display: 'flex', gap: 6, marginTop: 4 }}>
                {TOLERANCE_PRESETS.map((t) => (
                  <button
                    key={t}
                    onClick={() => {
                      setConfig({ ...config, requirement: { ...config.requirement, tolerance: t } })
                      setCustomTolerance('')
                    }}
                    style={{
                      background: !customTolerance && t === config.requirement.tolerance ? '#3b82f6' : '#2a2a2a',
                      color: '#fff',
                      padding: '4px 10px',
                      border: '1px solid #444',
                      borderRadius: 4,
                      fontSize: 12,
                    }}
                  >
                    {t === 0 ? '不允许误差' : `+${t}s`}
                  </button>
                ))}
              </div>
              <div style={{ marginTop: 6, display: 'flex', alignItems: 'center', gap: 6 }}>
                <span style={{ fontSize: 12, color: '#888' }}>自定义:</span>
                <input
                  type="number"
                  min={0}
                  max={10}
                  step={0.5}
                  value={customTolerance}
                  onChange={(e) => setCustomTolerance(e.target.value)}
                  placeholder={String(config.requirement.tolerance)}
                  style={{ width: 60, padding: '4px 8px', fontSize: 12 }}
                />
                <span style={{ fontSize: 12, color: '#888' }}>秒</span>
                <span style={{ fontSize: 12, color: '#34d399', marginLeft: 8 }}>
                  {effectiveTolerance === 0 ? '不允许误差' : `+${effectiveTolerance.toFixed(1)}s`}
                </span>
              </div>
            </>
          )}

          <button
            className="primary"
            disabled={!canRun}
            onClick={config.useAi ? runAiCut : runPureCut}
            style={{ width: '100%', marginTop: 20, padding: '14px', background: config.useAi ? '#8b5cf6' : '#10b981', color: 'white', border: 'none', borderRadius: 6, cursor: 'pointer', fontSize: 15, fontWeight: 'bold' }}
          >
            {running
              ? `处理中 ${progress.current}/${progress.total}`
              : `开始${config.useAi ? ' AI ' : ' '}裁切 (${videos.length} 个视频)`}
          </button>

          <h2 style={{ marginTop: 16, fontSize: 13, color: '#9ca3af' }}>
            实时日志 {logs.length > 0 && <span style={{ fontSize: 11 }}>({logs.length})</span>}
          </h2>
          <div
            ref={logBoxRef}
            style={{
              height: 240,
              overflowY: 'auto',
              background: '#0a0a0a',
              border: '1px solid #2a2a2a',
              borderRadius: 4,
              padding: 8,
              fontFamily: 'ui-monospace, SF Mono, Menlo, monospace',
              fontSize: 11,
              lineHeight: 1.4,
              color: '#d4d4d4',
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-all',
            }}
          >
            {logs.length === 0 && <div style={{ color: '#666' }}>等待运行…</div>}
            {logs.map((l, i) => (
              <div
                key={i}
                style={{
                  color: l.startsWith('[AI 分析]') ? '#a78bfa'
                    : l.startsWith('[AI 完成]') ? '#34d399'
                    : l.startsWith('[纯裁剪]') ? '#fbbf24'
                    : l.startsWith('✓') ? '#34d399'
                    : l.startsWith('✗') ? '#f87171'
                    : '#d4d4d4',
                }}
              >
                {l}
              </div>
            ))}
          </div>
        </section>

        {/* 右：视频列表 + 结果 */}
        <section className="panel scroll">
          <h2>输入视频</h2>
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>文件名</th>
                  <th>大小</th>
                </tr>
              </thead>
              <tbody>
                {videos.length === 0 && (
                  <tr>
                    <td colSpan={2} className="empty">请选择输入文件夹</td>
                  </tr>
                )}
                {videos.map((v) => (
                  <tr key={v.path}>
                    <td title={v.path}>{v.name}</td>
                    <td>{(v.size / 1024 / 1024).toFixed(1)} MB</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {results.length > 0 && (
            <div style={{ marginTop: 24 }}>
              <h2>处理结果</h2>
              {results.map((r, i) => (
                <div key={i} className={`result-item ${r.error ? 'err' : 'ok'}`} style={{ flexDirection: 'column', alignItems: 'flex-start' }}>
                  <div style={{ display: 'flex', width: '100%' }}>
                    <span>{r.error ? '✗' : '✓'}</span>
                    <span className="r-name" style={{ marginLeft: 8 }}>{r.source_name}</span>
                    <span className="meta" style={{ marginLeft: 'auto' }}>
                      {r.error ? r.error : `${r.segments.length} 段`}
                    </span>
                  </div>
                  {!r.error && r.segments.length > 0 && (
                    <div style={{ marginTop: 6, width: '100%' }}>
                      <table style={{ width: '100%', fontSize: 11 }}>
                        <thead>
                          <tr>
                            <th>文件</th>
                            <th>源区间</th>
                            <th>时长</th>
                          </tr>
                        </thead>
                        <tbody>
                          {r.segments.map((s, j) => (
                            <tr key={j}>
                              <td title={s.output_path} style={{ fontFamily: 'monospace' }}>
                                {s.output_path.split('/').pop()}
                              </td>
                              <td>{s.src_start.toFixed(2)}s ~ {s.src_end.toFixed(2)}s</td>
                              <td>{s.duration.toFixed(2)}s</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </section>
      </main>

      {/* AI 设置弹窗 */}
      {showSettings && (
        <div
          style={{
            position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.6)',
            display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 100,
          }}
          onClick={() => setShowSettings(false)}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            style={{
              background: '#1f1f1f', padding: 24, borderRadius: 8,
              minWidth: 620, maxWidth: 760, color: '#fff', maxHeight: '90vh', overflowY: 'auto',
            }}
          >
            <h2 style={{ marginTop: 0 }}>⚙ AI 设置</h2>

            <p style={{ fontSize: 12, color: '#888', marginTop: 0, marginBottom: 16 }}>
              以下是 AI 智能裁切的连接配置（仅在「AI 裁切」模式下生效）。
            </p>

            {config.useAi && (
              <>
                <h3 style={{ marginTop: 0, color: '#9ca3af', fontSize: 13 }}>连接</h3>
                <label>Base URL</label>
                <input
                  type="text" value={config.baseUrl}
                  onChange={(e) => setConfig({ ...config, baseUrl: e.target.value })}
                  style={{ width: '100%', marginBottom: 8 }}
                />
                <label>API Key</label>
                <input
                  type="password" value={config.apiKey}
                  onChange={(e) => setConfig({ ...config, apiKey: e.target.value })}
                  style={{ width: '100%', marginBottom: 8 }}
                />
                <label>模型名</label>
                <input
                  type="text" value={config.model}
                  onChange={(e) => setConfig({ ...config, model: e.target.value })}
                  style={{ width: '100%', marginBottom: 8 }}
                />
                <div style={{ display: 'flex', gap: 12, marginBottom: 8 }}>
                  <div style={{ flex: 1 }}>
                    <label style={{ fontSize: 12, color: '#9ca3af' }}>分析抽帧（帧/分钟）</label>
                    <div style={{ display: 'flex', gap: 4, marginTop: 4 }}>
                      {[30, 60, 90, 120].map((v) => (
                        <button
                          key={v}
                          onClick={() => setConfig({ ...config, fps: v / 60 })}
                          style={{
                            background: Math.round(config.fps * 60) === v ? '#3b82f6' : '#2a2a2a',
                            color: '#fff', padding: '6px 12px', border: '1px solid #444', borderRadius: 4, fontSize: 12, flex: 1,
                          }}
                        >{v}</button>
                      ))}
                    </div>
                    <div style={{ fontSize: 10, color: '#666', marginTop: 2 }}>
                      ≈ 每 {(60 / Math.max(config.fps * 60, 1)).toFixed(1)}s 抽 1 帧
                    </div>
                  </div>
                  <div style={{ flex: 1 }}>
                    <label style={{ fontSize: 12, color: '#9ca3af' }}>分辨率（清晰度）</label>
                    <select
                      value={config.frameMaxSize}
                      onChange={(e) => setConfig({ ...config, frameMaxSize: parseInt(e.target.value) })}
                      style={{ width: '100%', padding: '6px 8px', marginTop: 4, background: '#2a2a2a', color: '#fff', border: '1px solid #444', borderRadius: 4 }}
                    >
                      <option value={240}>极快 240p</option>
                      <option value={360}>极速 360p</option>
                      <option value={480}>标准 480p ★</option>
                      <option value={720}>高清 720p</option>
                      <option value={1080}>原画 1080p</option>
                    </select>
                    <div style={{ fontSize: 10, color: '#666', marginTop: 2 }}>
                      越高越清晰，AI 越准但更慢
                    </div>
                  </div>
                </div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 16 }}>
                  <button onClick={testAI}>测试连接</button>
                  <span style={{ fontSize: 12, color: testStatus.startsWith('OK') ? '#34d399' : '#fbbf24' }}>
                    {testStatus}
                  </span>
                </div>

                <h3 style={{ color: '#9ca3af', fontSize: 13 }}>切段需求（动态字段）</h3>
                <div style={{ fontSize: 11, color: '#9ca3af', marginBottom: 8 }}>
                  当前模板：
                  {(() => {
                    const t = templateList.templates.find((x) => x.id === config.currentTemplateId)
                    return t ? <strong style={{ color: '#34d399' }}>{t.name}</strong>
                      : <em style={{ color: '#fbbf24' }}>未保存的自由编辑</em>
                  })()}
                </div>

                <div style={{ display: 'flex', flexDirection: 'column', gap: 8, marginBottom: 12 }}>
                  {config.requirement.fields.map((f) => (
                    <div key={f.id} style={{ background: '#2a2a2a', padding: 8, borderRadius: 4, display: 'flex', gap: 8 }}>
                      <input
                        type="text"
                        value={f.key}
                        onChange={(e) => updateField(f.id, { key: e.target.value })}
                        placeholder="字段名"
                        style={{ width: 140, padding: '4px 8px', fontSize: 12 }}
                      />
                      <textarea
                        rows={2}
                        value={f.value}
                        onChange={(e) => updateField(f.id, { value: e.target.value })}
                        placeholder="字段内容（拼到 prompt 里）"
                        style={{ flex: 1, padding: '4px 8px', fontSize: 12, resize: 'vertical' }}
                      />
                      <button
                        onClick={() => removeField(f.id)}
                        style={{ padding: '4px 8px', fontSize: 11, color: '#f87171' }}
                        title="删除字段"
                      >×</button>
                    </div>
                  ))}
                </div>
                <button onClick={addField} style={{ marginBottom: 16, fontSize: 12 }}>
                  + 添加字段
                </button>

                <h3 style={{ color: '#9ca3af', fontSize: 13 }}>模板管理</h3>
                <div style={{ fontSize: 11, color: '#9ca3af', marginBottom: 8 }}>
                  当前编辑：
                  <input
                    type="text"
                    value={editingTemplateName}
                    onChange={(e) => setEditingTemplateName(e.target.value)}
                    placeholder="模板名"
                    style={{ marginLeft: 8, width: 180, padding: '2px 6px', fontSize: 12 }}
                  />
                </div>
                <div style={{ display: 'flex', gap: 6, marginBottom: 12 }}>
                  <button onClick={newTemplate} style={{ fontSize: 12 }}>🆕 新模板</button>
                  <button onClick={saveAsNew} style={{ fontSize: 12 }}>💾 保存为新</button>
                  <button
                    onClick={overwriteCurrent}
                    disabled={!config.currentTemplateId}
                    style={{ fontSize: 12, opacity: config.currentTemplateId ? 1 : 0.4 }}
                    title={!config.currentTemplateId ? '当前未载入模板' : ''}
                  >📝 覆盖当前</button>
                </div>

                <div style={{ maxHeight: 200, overflowY: 'auto', border: '1px solid #2a2a2a', borderRadius: 4, padding: 6 }}>
                  {templateList.templates.length === 0 && <div style={{ color: '#666', fontSize: 12 }}>加载中…</div>}
                  {templateList.templates.map((t) => (
                    <div key={t.id} style={{
                      display: 'flex', alignItems: 'center', gap: 8, padding: '4px 6px',
                      background: t.id === config.currentTemplateId ? '#1e3a5f' : 'transparent',
                      borderRadius: 3, marginBottom: 2,
                    }}>
                      <span style={{ flex: 1, fontSize: 12 }}>
                        {t.builtin && '📌 '}
                        {t.name}
                        <span style={{ color: '#666', fontSize: 10, marginLeft: 6 }}>
                          ({t.requirement.fields.length} 字段)
                        </span>
                      </span>
                      <button onClick={() => loadTemplate(t)} style={{ fontSize: 11, padding: '2px 8px' }}>载入</button>
                      <button
                        onClick={() => deleteTpl(t.id)}
                        disabled={t.builtin}
                        style={{ fontSize: 11, padding: '2px 8px', color: t.builtin ? '#666' : '#f87171' }}
                      >删除</button>
                    </div>
                  ))}
                </div>
              </>
            )}

            <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end', marginTop: 16 }}>
              <button onClick={() => { setConfig(DEFAULT_CONFIG); setCustomDuration(''); setCustomTolerance(''); setEditingTemplateName('未命名') }}>
                恢复默认
              </button>
              <button onClick={() => setShowSettings(false)}>取消</button>
              <button
                className="primary"
                onClick={async () => {
                  await saveConfig(configRef.current)
                  setShowSettings(false)
                }}
              >
                保存
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
