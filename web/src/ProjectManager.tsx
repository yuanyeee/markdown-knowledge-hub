import { isTauri } from '@tauri-apps/api/core';
import { useEffect, useState } from 'react';
import { read, write as sendCommand } from './transport';
import { ConfirmRemove } from './ConfirmRemove';
import { displayPath, managedPath, validateProjectId } from './folders';

export type Project = {
  id: string;
  name: string;
  root: string;
  output: string;
  legacy_output: boolean;
};

type Retired = {
  key: string;
  project: string;
  path: string;
};

type ProjectKind = 'markdown' | 'oml' | 'files' | 'md-files' | 'convert';

type Props = {
  current?: Project;
  enabled: boolean;
  folderPicker: boolean;
  onChanged: (preferred?: string) => Promise<void>;
  onBusy: (busy: boolean) => void;
};

export function ProjectManager({
  current,
  enabled,
  folderPicker,
  onChanged,
  onBusy,
}: Props) {
  const [kind, setKind] = useState<ProjectKind>('oml');
  const [selectedFiles, setSelectedFiles] = useState<string[]>([]);
  const oml = kind === 'oml' || kind === 'files';
  const filesMode = kind === 'files' || kind === 'md-files' || kind === 'convert';
  const [mode, setMode] = useState<'create' | 'open' | null>(null);
  const [id, setId] = useState('');
  const [customId, setCustomId] = useState('');
  const [removeTarget, setRemoveTarget] = useState<Project | null>(null);
  const [cleanupTarget, setCleanupTarget] = useState<Retired | null>(null);
  const [name, setName] = useState('');
  const [root, setRoot] = useState('');
  const [output, setOutput] = useState('');
  const [lastOutput, setLastOutput] = useState('');
  const [existing, setExisting] = useState('');
  const [newInput, setNewInput] = useState('');
  const [newOutput, setNewOutput] = useState('');
  const [retired, setRetired] = useState<Retired[]>([]);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');
  const [warning, setWarning] = useState('');

  const trimmedCustomId = customId.trim();
  const effectiveId = trimmedCustomId || id;
  const customIdError = trimmedCustomId ? validateProjectId(trimmedCustomId) : null;

  async function command(operation: string, args: Record<string, unknown> = {}) {
    const result = await sendCommand(operation, args);
    if (typeof result.warning === 'string') {
      setWarning(result.warning);
    }
    return result;
  }

  async function refreshSettings() {
    const [settings, old] = await Promise.all([
      read<{ last_output: string | null }>('output-settings'),
      read<Retired[]>('retired-outputs'),
    ]);
    setLastOutput(settings.last_output || '');
    setRetired(old);
  }

  useEffect(() => {
    if (enabled) {
      void refreshSettings().catch((e) => setError(String(e)));
    }
  }, [enabled]);

  useEffect(() => {
    setNewInput(current?.root || '');
    setNewOutput('');
  }, [current?.id, current?.root, current?.output]);

  async function perform(action: () => Promise<void>, label: string) {
    setPending(true);
    onBusy(true);
    setError('');
    setWarning('');
    setMessage(label);
    try {
      await action();
      await refreshSettings();
    } catch (e) {
      setError(String(e));
      setMessage('');
    } finally {
      setPending(false);
      onBusy(false);
    }
  }

  function picker(setter: (value: string) => void, purpose: string) {
    void perform(async () => {
      const result = await command('pick-folder', { purpose });
      if (typeof result.path === 'string') {
        setter(result.path);
      }
      setMessage('');
    }, 'フォルダー選択画面を開いています…');
  }

  function folder(
    label: string,
    value: string,
    setter: (value: string) => void,
    placeholder: string,
  ) {
    return (
      <label>
        {label}
        <div className="folder-input">
          <input
            required
            aria-label={label}
            value={displayPath(value)}
            disabled={pending}
            onChange={(e) => setter(e.target.value)}
            placeholder={placeholder}
          />
          {folderPicker && (
            <button
              type="button"
              disabled={pending}
              onClick={() => picker(setter, label)}
            >
              {label}を選択
            </button>
          )}
        </div>
      </label>
    );
  }

  if (!enabled) {
    return (
      <p className="admin-hint">
        閲覧モードです。プロジェクトの追加・更新は管理者が行います。
      </p>
    );
  }

  return (
    <section className="manager" aria-label="プロジェクト管理">
      <div className="manager-actions">
        <button
          disabled={pending}
          onClick={() => {
            if (mode === 'create') {
              setMode(null);
              return;
            }
            setMode('create');
            setKind('oml');
            setSelectedFiles([]);
            setId(`project-${crypto.randomUUID()}`);
            setCustomId('');
            setName('');
            setRoot('');
            setOutput(lastOutput);
            setError('');
            setMessage('');
            setWarning('');
          }}
        >
          新規プロジェクト
        </button>
        <button
          disabled={pending}
          onClick={() => {
            setMode(mode === 'open' ? null : 'open');
            setError('');
          }}
        >
          既存プロジェクトを開く
        </button>
        <button
          disabled={!current || pending || mode !== null}
          onClick={() =>
            void perform(async () => {
              await command('rebuild', { project: current!.id });
              await onChanged(current!.id);
              setMessage('Wiki・検索索引・リンク関係を再作成しました。');
            }, '入力を読み直して再作成しています…')
          }
        >
          再作成
        </button>
        {isTauri() && current?.root.includes('File-Wiki-') && (
          <button
            disabled={pending || mode !== null}
            onClick={() =>
              void perform(async () => {
                const picked = await command('pick-convert-files');
                const files = Array.isArray(picked.paths)
                  ? (picked.paths as string[])
                  : [];
                if (!files.length || !current) return;
                await command('add-converted', {
                  project: current.id,
                  files,
                });
                await onChanged(current.id);
                setMessage('選んだファイルをWikiへ追加しました。');
              }, 'ファイルを変換してWikiへ追加しています…')
            }
          >
            ファイルをWikiへ追加
          </button>
        )}
        <button
          className="danger"
          disabled={!current || pending || mode !== null}
          onClick={() => {
            if (current) setRemoveTarget(current);
          }}
        >
          プロジェクトを削除
        </button>
      </div>

      {mode === 'create' && (
        <form
          className="project-form"
          onSubmit={(e) => {
            e.preventDefault();
            if (customIdError) return;
            const targetId = effectiveId;
            void perform(async () => {
              await command('create', {
                id: targetId,
                name,
                root,
                output,
                oml,
                ...(kind === 'files'
                  ? { oml_files: selectedFiles }
                  : kind === 'md-files'
                    ? { md_files: selectedFiles }
                    : kind === 'convert'
                      ? { convert_files: selectedFiles }
                      : {}),
              });
              await onChanged(targetId);
              setMode(null);
              setCustomId('');
              setName('');
              setRoot('');
              setSelectedFiles([]);
              setMessage(
                '指定した出力先にWiki・検索索引・リンク関係を作成しました。',
              );
            }, 'Markdownを解析してWikiとリンク関係を作成しています…');
          }}
        >
          <h2>新規プロジェクト</h2>
          {current && (
            <p className="creation-context">
              現在は「{current.name}
              」を閲覧中です。以下は別のプロジェクトの新規登録です。作成が完了すると新しいWikiへ切り替わり、キャンセルすると現在の閲覧を続けます。
            </p>
          )}
          <label>
            プロジェクト名
            <input
              required
              maxLength={100}
              value={name}
              disabled={pending}
              onChange={(e) => setName(e.target.value)}
              placeholder="例：販売システム"
            />
          </label>
          <label>
            プロジェクトの種類
            <select
              aria-label="プロジェクトの種類"
              value={kind}
              disabled={pending}
              onChange={(e) => {
                setKind(e.target.value as ProjectKind);
                setRoot('');
                setSelectedFiles([]);
              }}
            >
              <option value="oml">OML仕様書からWikiを生成</option>
              <option value="markdown">
                Markdownフォルダー（サブフォルダーを含む）
              </option>
              {isTauri() && (
                <>
                  <option value="md-files">Markdownファイルを複数選択</option>
                  <option value="files">OMLファイルを選んでWikiを生成</option>
                  <option value="convert">ファイルを選んでWikiを生成</option>
                </>
              )}
            </select>
          </label>
          <label>
            識別ID（任意・未入力なら自動採番）
            <input
              maxLength={80}
              value={customId}
              disabled={pending}
              onChange={(e) => setCustomId(e.target.value.toLowerCase())}
              placeholder={`例：sales-spec（未入力時: ${id}）`}
            />
          </label>
          {customIdError && (
            <p role="alert" className="error">
              {customIdError}
            </p>
          )}
          {kind === 'oml' && (
            <p className="hint">
              OML Utilitiesで出力したMarkdownのフォルダーを選びます。生成したWikiは出力の親フォルダー内に別フォルダーとして保存します。
            </p>
          )}
          {filesMode ? (
            <>
              <button
                type="button"
                disabled={pending}
                onClick={() =>
                  void perform(async () => {
                    const result = await command(
                      kind === 'files'
                        ? 'pick-oml-files'
                        : kind === 'convert'
                          ? 'pick-convert-files'
                          : 'pick-md-files',
                    );
                    if (Array.isArray(result.paths)) {
                      setSelectedFiles(result.paths as string[]);
                    }
                    setMessage('');
                  }, 'ファイルを選択しています…')
                }
              >
                {kind === 'files'
                  ? 'OML'
                  : kind === 'convert'
                    ? '変換する'
                    : 'Markdown'}
                ファイルを選択（複数可）
              </button>
              <p>{selectedFiles.length}件選択</p>
              <ul>
                {selectedFiles.map((path) => (
                  <li key={path}>{path.split(/[\\/]/).pop()}</li>
                ))}
              </ul>
              <p className="hint">
                CtrlまたはShiftを押しながら複数選べます。
                {kind === 'files'
                  ? 'OML Utilitiesのoml.exeをアプリと同じフォルダーに置いてください。'
                  : kind === 'convert'
                    ? 'docx、xlsx、pptx、html、txt、csv を OKF の Wiki に変換します。元のファイルは変更しません。'
                    : '選んだファイルのコピーを索引化します。'}
              </p>
            </>
          ) : (
            <>
              {folder(
                oml ? 'OML仕様書フォルダー' : '入力フォルダー',
                root,
                setRoot,
                'D:\\Specifications\\SystemA',
              )}
              <p className="hint">
                お客様のMarkdownがある場所です。配下の.mdを読み取り、原本は変更しません。
              </p>
            </>
          )}

          {folder(
            '出力の親フォルダー',
            output,
            setOutput,
            'D:\\MarkdownHubOutputs',
          )}
          {lastOutput && output === lastOutput && (
            <p className="hint">
              前回選択した親フォルダーを候補として入力しています。今回の保存場所に変更できます。
            </p>
          )}
          <p className="hint">
            親フォルダーを選ぶと、その中に今回専用の保存先を自動作成します。既存プロジェクトのproject.jsonがあるフォルダーは選びません。
          </p>
          <div className="output-preview">
            <strong>今回、新しく作成する保存先</strong>
            <output aria-label="新規プロジェクトの生成データ保存先">
              {output.trim()
                ? managedPath(output, effectiveId)
                : '出力の親フォルダーを選択してください'}
            </output>
          </div>
          <p className="hint">
            出力には閲覧用の本文コピー・検索索引・リンク情報が含まれます。専用の子フォルダーを作り、親フォルダーの既存ファイルは変更しません。
          </p>
          <button
            disabled={
              pending ||
              !name.trim() ||
              (filesMode ? !selectedFiles.length : !root.trim()) ||
              !output.trim() ||
              Boolean(customIdError)
            }
            type="submit"
          >
            作成してWikiを開く
          </button>
          <button
            disabled={pending}
            type="button"
            onClick={() => setMode(null)}
          >
            キャンセル
          </button>
        </form>
      )}

      {mode === 'open' && (
        <form
          className="project-form"
          onSubmit={(e) => {
            e.preventDefault();
            void perform(async () => {
              const result = await command('open-output', { output: existing });
              await onChanged(String(result.id));
              setMode(null);
              setMessage('保存済みの生成データからプロジェクトを開きました。');
            }, '出力フォルダーを確認しています…');
          }}
        >
          <h2>既存プロジェクトを開く</h2>
          {folder(
            '既存プロジェクトの保存先（project.jsonがある場所）',
            existing,
            setExisting,
            'D:\\MarkdownHubOutputs\\MarkdownKnowledgeHub\\project-…',
          )}
          <p className="hint">
            新規作成時の親フォルダーではなく、その下のMarkdownKnowledgeHub内にあるプロジェクト専用フォルダーを選びます。project.jsonが目印です。入力が見つからなくても保存済みのWikiを閲覧できます。同じIDの登録は上書きしません。
          </p>
          <button type="submit" disabled={pending || !existing.trim()}>
            開く
          </button>
          <button
            type="button"
            disabled={pending}
            onClick={() => setMode(null)}
          >
            キャンセル
          </button>
        </form>
      )}

      {current && !mode && (
        <details className="project-settings" key={current.id}>
          <summary>プロジェクトの保存先・設定</summary>
          <dl>
            <dt>識別ID</dt>
            <dd>{current.id}</dd>
            <dt>入力フォルダー（読み取り専用）</dt>
            <dd>{displayPath(current.root)}</dd>
            <dt>生成データ保存先{current.legacy_output ? '（従来形式）' : ''}</dt>
            <dd>{displayPath(current.output)}</dd>
          </dl>
          <form
            className="project-form"
            onSubmit={(e) => {
              e.preventDefault();
              void perform(async () => {
                await command('change-input', {
                  project: current.id,
                  root: newInput,
                });
                await onChanged(current.id);
                setMessage(
                  '入力フォルダーを変更しました。「再作成」で最新の内容を反映できます。監視中は自動で読み直します。',
                );
              }, '入力フォルダーを変更しています…');
            }}
          >
            {folder(
              '新しい入力フォルダー',
              newInput,
              setNewInput,
              'Markdownがある場所',
            )}
            <p className="hint">
              入力の場所が変わった場合に指定します。保存済みのWikiは「再作成」まで保持します。
            </p>
            <button
              disabled={
                pending || !newInput.trim() || newInput === current.root
              }
            >
              入力先を変更
            </button>
          </form>
          <form
            className="project-form"
            onSubmit={(e) => {
              e.preventDefault();
              void perform(async () => {
                await command('move-output', {
                  project: current.id,
                  output: newOutput,
                });
                await onChanged(current.id);
                setNewOutput('');
                setMessage(
                  '出力先を変更しました。旧出力は保管しています。新しい保存先での閲覧を確認後に整理できます。',
                );
              }, '生成データをコピー・検証して保存先を切り替えています…');
            }}
          >
            {folder(
              '移動先の親フォルダー',
              newOutput,
              setNewOutput,
              '移動先の親フォルダー',
            )}
            <p className="hint">
              変更後：
              {newOutput.trim()
                ? managedPath(newOutput, current.id)
                : '親フォルダーを選択してください'}
              。検証後に切り替え、旧出力は自動削除しません。
            </p>
            <button disabled={pending || !newOutput.trim()}>
              出力先を変更
            </button>
          </form>
        </details>
      )}

      {!mode && retired.length > 0 && (
        <details className="project-settings">
          <summary>保管中の旧出力（{retired.length}件）</summary>
          <p className="hint">
            移行前のコピーです。新しい保存先で正常に閲覧できることを確認してから削除してください。
          </p>
          {retired.map((old) => (
            <div className="retired-output" key={old.key}>
              <span>
                {old.project}
                <br />
                {displayPath(old.path)}
              </span>
              <button
                className="danger"
                disabled={pending}
                onClick={() => setCleanupTarget(old)}
              >
                旧出力を削除
              </button>
            </div>
          ))}
        </details>
      )}

      {removeTarget && (
        <ConfirmRemove
          name={removeTarget.name}
          onCancel={() => setRemoveTarget(null)}
          onConfirm={() => {
            const target = removeTarget;
            setRemoveTarget(null);
            void perform(async () => {
              await command('remove', { project: target.id });
              await onChanged();
              setMessage(
                '登録と現在の専用出力を削除しました。入力Markdownと親フォルダーの既存ファイルは残っています。',
              );
            }, '専用出力を削除しています…');
          }}
        />
      )}

      {cleanupTarget && (
        <ConfirmRemove
          name={displayPath(cleanupTarget.path)}
          retired
          onCancel={() => setCleanupTarget(null)}
          onConfirm={() => {
            const target = cleanupTarget;
            setCleanupTarget(null);
            void perform(async () => {
              await command('cleanup-output', { key: target.key });
              setMessage(
                '保管中の旧出力を削除しました。現在の出力と入力Markdownは残っています。',
              );
            }, '旧出力を削除しています…');
          }}
        />
      )}

      {message && <p role="status">{message}</p>}
      {warning && (
        <p className="error" role="alert">
          {warning}
        </p>
      )}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
