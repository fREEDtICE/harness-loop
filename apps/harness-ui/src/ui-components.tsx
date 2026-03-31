import { useRef, useEffect } from "react";
import {
  MDXEditor,
  headingsPlugin,
  listsPlugin,
  quotePlugin,
  markdownShortcutPlugin,
  thematicBreakPlugin,
  toolbarPlugin,
  BoldItalicUnderlineToggles,
  ListsToggle,
  UndoRedo,
  type MDXEditorMethods,
} from "@mdxeditor/editor";
import "@mdxeditor/editor/style.css";

export function PromptEditorModal({
  title,
  markdown,
  onChange,
  onClose,
}: {
  title: string;
  markdown: string;
  onChange: (value: string) => void;
  onClose: () => void;
}) {
  const ref = useRef<MDXEditorMethods>(null);

  useEffect(() => {
    ref.current?.setMarkdown(markdown);
  }, [markdown]);

  return (
    <div className="prompt-modal-overlay">
      <div className="prompt-modal">
        <div className="prompt-modal-header">
          <h3>{title}</h3>
          <button className="prompt-modal-close" onClick={onClose}>×</button>
        </div>
        <div className="prompt-modal-body">
          <MDXEditor
            ref={ref}
            className="prompt-mdx-editor"
            contentEditableClassName="prompt-mdx-content"
            markdown={markdown}
            onChange={onChange}
            plugins={[
              headingsPlugin(),
              listsPlugin(),
              quotePlugin(),
              thematicBreakPlugin(),
              markdownShortcutPlugin(),
              toolbarPlugin({
                toolbarContents: () => (
                  <>
                    <UndoRedo />
                    <BoldItalicUnderlineToggles />
                    <ListsToggle />
                  </>
                ),
              }),
            ]}
          />
        </div>
      </div>
    </div>
  );
}

export function PromptEditor({
  testid,
  title,
  value,
  onChange,
}: {
  testid: string;
  title: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <section className="prompt-editor" data-testid={testid}>
      <div className="prompt-editor-header">
        <h3>{title}</h3>
      </div>
      <textarea data-testid={`${testid}-textarea`} value={value} onChange={(event) => onChange(event.target.value)} rows={9} />
    </section>
  );
}

export function StatCard({
  testid,
  label,
  value,
  detail,
}: {
  testid: string;
  label: string;
  value: string;
  detail: string;
}) {
  return (
    <article className="stat-card" data-testid={testid}>
      <span>{label}</span>
      <strong>{value}</strong>
      <p>{detail}</p>
    </article>
  );
}

export function StageRow({ testid, heading, text }: { testid?: string; heading: string; text: string }) {
  return (
    <div className="stage-row" data-testid={testid}>
      <strong>{heading}</strong>
      <span>{text}</span>
    </div>
  );
}
