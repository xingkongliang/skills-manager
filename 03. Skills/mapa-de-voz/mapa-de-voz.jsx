
import { useState } from "react";
import { Radar, RadarChart, PolarGrid, PolarAngleAxis, ResponsiveContainer } from "recharts";

const G = {
  bg: '#0C0B0F', card: '#131218', border: '#252330',
  gold: '#D4A843', goldFaint: 'rgba(212,168,67,0.1)',
  goldLight: '#E8C876', text: '#F2EDE4', dim: '#8A8490',
  accent: '#3D3560',
};

const QUESTIONS = [
  {
    q: "Cuando escribes, ¿qué te impulsa más?",
    hint: "Tu motivación define el corazón de tu voz",
    opts: [
      { t: "Contar una historia que aún no existe en el mundo", s: [3,5,3,4,4] },
      { t: "Entender algo que me confunde o me duele", s: [5,3,2,5,3] },
      { t: "Provocar algo en quien lee: asombro, incomodidad, ternura", s: [4,4,4,3,5] },
      { t: "Capturar un instante antes de que desaparezca para siempre", s: [4,4,5,4,2] },
    ]
  },
  {
    q: "¿Cómo es tu relación natural con las palabras?",
    hint: "Aquí vive tu ritmo más profundo",
    opts: [
      { t: "Las busco con obsesión hasta encontrar la exacta", s: [3,4,3,3,4] },
      { t: "Las dejo fluir sin censura y las corrijo después", s: [4,5,5,4,4] },
      { t: "Las elijo primero por su sonido, luego por su significado", s: [3,5,5,3,5] },
      { t: "Uso las mínimas posibles; el silencio también escribe", s: [4,4,4,5,3] },
    ]
  },
  {
    q: "Si tu escritura fuera un género musical, sería...",
    hint: "La metáfora revela lo que las definiciones no pueden",
    opts: [
      { t: "Jazz — improvisada, libre, con silencios que hablan", s: [3,5,4,4,5] },
      { t: "Música clásica — arquitectura precisa, nada al azar", s: [3,3,3,3,2] },
      { t: "Rock — cruda, directa, sin pedir permiso", s: [5,4,5,3,5] },
      { t: "Electrónica experimental — capas, texturas, lo invisible", s: [4,5,4,5,5] },
    ]
  },
  {
    q: "¿Cuándo sientes que tu voz \"no es tuya\"?",
    hint: "Nombrar el obstáculo ya es empezar a superarlo",
    opts: [
      { t: "Cuando sueno demasiado a un escritor que admiro", s: [3,2,3,3,2] },
      { t: "Cuando el texto suena correcto pero frío, casi académico", s: [2,2,2,3,1] },
      { t: "Cuando me censuro antes de terminar una idea arriesgada", s: [3,3,3,4,2] },
      { t: "Cuando cambio de tono en el mismo texto sin darme cuenta", s: [3,3,2,3,3] },
    ]
  },
  {
    q: "¿Qué escritura te hace sentir más vivo/a al leerla?",
    hint: "Lo que admiras en otros es la señal de lo que quieres crear",
    opts: [
      { t: "Una sola oración que te golpea en el estómago", s: [5,4,4,4,4] },
      { t: "Historias que te desorientan y de pronto todo cobra sentido", s: [4,5,4,5,5] },
      { t: "Textos que mezclan géneros y rompen reglas sin disculparse", s: [4,5,5,5,5] },
      { t: "Escritura tan honesta que te da vergüenza leerla y no puedes parar", s: [5,4,3,5,4] },
    ]
  },
];

const DIMS = ["Intensidad", "Originalidad", "Ritmo", "Perspectiva", "Riesgo"];

const PROFILES = [
  {
    name: "La Voz del Fuego Interior",
    icon: "◈",
    color: '#E8836A',
    desc: "Escribes desde las entrañas. Tu impulso es emocional antes que intelectual, y eso es un don que no se puede enseñar. Tu riesgo: la intensidad sin respiración agota al lector. Tu superpoder: haces sentir, y eso es casi todo lo que importa en la literatura.",
    exs: [
      { title: "La escena sin emociones", body: "Escribe una escena de alta carga emocional usando solo objetos y acciones físicas. Prohibido mencionar sentimientos o pensamientos. Oblígate a mostrar, nunca a decir." },
      { title: "La segunda persona íntima", body: "Toma tu texto más frío y reescríbelo en segunda persona, tiempo presente: 'Tú abres la puerta y...'. Observa cómo cambia la temperatura emocional de inmediato." },
      { title: "Diez minutos sin parar", body: "Pon un temporizador. Escribe sin levantar la pluma, sin borrar, sin releer. Solo seguir adelante. Tu voz más honesta vive en los primeros cuatro minutos." }
    ]
  },
  {
    name: "La Voz del Borde",
    icon: "✦",
    color: '#7B9EE0',
    desc: "Tiendes hacia lo que aún no existe. Mezclas géneros, buscas el ángulo que nadie ha tomado. Tu riesgo: la originalidad sin raíz puede volverse artificio vacío. Tu superpoder: puedes crear mundos que nadie imaginó antes que tú.",
    exs: [
      { title: "El formato imposible", body: "Elige un formato que nunca usarías (receta de cocina, instrucciones de uso, testamento, aviso de pérdida) y escribe algo profundamente personal dentro de ese molde." },
      { title: "El objeto que observa", body: "Toma una de tus piezas y reescribe su escena central desde el punto de vista de un objeto inanimado que todo lo ve y nada puede cambiar." },
      { title: "El texto transmedia", body: "Un solo argumento, tres lenguajes: comienza en prosa narrativa, continúa como diálogo teatral, termina como poema. Sin explicar los cambios de registro." }
    ]
  },
  {
    name: "La Voz del Pulso",
    icon: "〜",
    color: '#6EC4A8',
    desc: "Tu prosa tiene música, aunque a veces no lo sepas todavía. El ritmo es tu instinto más confiable. Tu riesgo: cuando te preocupas por el qué decir, pierdes la cadencia. Tu superpoder: cuando confías en el sonido, el sentido llega solo.",
    exs: [
      { title: "Lee en voz alta, siempre", body: "A partir de hoy: todo lo que escribas, léelo en voz alta antes de guardarlo. Si tropiezas en alguna frase, reescríbela. Si fluye sin esfuerzo, déjala tal cual." },
      { title: "El experimento de longitud", body: "Escribe el mismo párrafo dos veces: una con frases muy cortas (4-6 palabras), otra con una sola frase larga y subordinada. Siente la diferencia en el cuerpo." },
      { title: "Transcripción viva", body: "Graba 5 minutos de una conversación real y transcríbela con atención. Analiza los ritmos, las interrupciones, las repeticiones. Úsalos en tu próxima ficción." }
    ]
  },
  {
    name: "La Voz del Testigo Íntimo",
    icon: "◎",
    color: '#C4837A',
    desc: "Observas el mundo interior con una precisión poco común. Escribes desde adentro hacia afuera. Tu riesgo: lo muy privado puede volverse opaco para quien lee. Tu superpoder: cuando encuentras el detalle concreto detrás de la emoción, lo universal aparece solo.",
    exs: [
      { title: "El distanciamiento íntimo", body: "Escribe sobre algo muy personal... pero en tercera persona, como si fuera un personaje de novela que no eres tú. Observa lo que ves desde esa distancia nueva." },
      { title: "La emoción prohibida", body: "Elige la emoción que más evitas poner en papel. Escríbela durante 15 minutos sin detenerte. No la expliques: muéstrala en un único momento concreto y físico." },
      { title: "El detalle universal", body: "Toma una experiencia íntima y busca el único detalle sensorial que la haría reconocible para un completo extraño que jamás vivió algo parecido." }
    ]
  },
  {
    name: "La Voz del Saltador",
    icon: "↗",
    color: '#A889D4',
    desc: "Tienes instinto para el peligro creativo. Sabes cuándo una idea es demasiado segura. Tu riesgo: el riesgo por el riesgo mismo es otra trampa. La valentía más difícil es ser simple cuando todo te pide ser complejo. Tu superpoder: puedes ir donde otros no se atreven.",
    exs: [
      { title: "El texto que más miedo te da", body: "Escribe el texto que más miedo te daría mostrar a alguien. Guárdalo. No lo publiques todavía. Léelo una semana después y observa qué sientes entonces." },
      { title: "La versión radicalmente simple", body: "Toma tu pieza más arriesgada o compleja y reescríbela con lenguaje directo, sin metáforas, con estructura lineal. ¿Qué pierdes? ¿Qué sorprendentemente ganas?" },
      { title: "El permiso de ser mediocre", body: "Escribe algo a propósito malo: sin esfuerzo, sin ambición, sin presión de brillar. Observa qué aparece cuando no hay expectativa. Ahí suele vivir la voz más verdadera." }
    ]
  },
];

const optStyle = (sel) => ({
  padding: '13px 16px', marginBottom: '9px',
  border: `1px solid ${sel ? G.gold : G.border}`,
  background: sel ? G.goldFaint : G.card,
  cursor: 'pointer', borderRadius: '3px',
  fontSize: '0.87rem', lineHeight: 1.65,
  color: sel ? G.goldLight : G.text,
  transition: 'all 0.17s',
});

const btnPrimary = (active) => ({
  background: active ? G.gold : G.border,
  color: active ? '#0C0B0F' : G.dim,
  border: 'none', padding: '11px 26px',
  fontFamily: "'Lora', serif", fontSize: '0.87rem',
  cursor: active ? 'pointer' : 'not-allowed',
  opacity: active ? 1 : 0.5, borderRadius: '2px',
  marginTop: '6px', letterSpacing: '0.04em',
  transition: 'all 0.18s',
});

const FONTS = `@import url('https://fonts.googleapis.com/css2?family=Cormorant+Garamond:ital,wght@0,400;0,600;1,400&family=Lora:ital,wght@0,400;1,400&display=swap');
* { box-sizing: border-box; margin: 0; padding: 0; }
::-webkit-scrollbar { width: 4px; }
::-webkit-scrollbar-track { background: #0C0B0F; }
::-webkit-scrollbar-thumb { background: #252330; }`;

export default function VozMap() {
  const [phase, setPhase] = useState("intro");
  const [qIdx, setQIdx] = useState(0);
  const [answers, setAnswers] = useState([]);
  const [sel, setSel] = useState(null);
  const [vis, setVis] = useState(true);

  const go = (fn) => {
    setVis(false);
    setTimeout(() => { fn(); setVis(true); }, 220);
  };

  const next = () => {
    if (!sel) return;
    const ans = [...answers, sel];
    setAnswers(ans);
    setSel(null);
    qIdx + 1 < QUESTIONS.length
      ? go(() => setQIdx(i => i + 1))
      : go(() => setPhase("results"));
  };

  const restart = () => go(() => {
    setPhase("intro"); setQIdx(0); setAnswers([]); setSel(null);
  });

  const totals = DIMS.map((_, i) => answers.reduce((s, a) => s + (a.s[i] || 0), 0));
  const norm = totals.map(v => Math.round((v / (QUESTIONS.length * 5)) * 100));
  const domIdx = totals.length ? totals.indexOf(Math.max(...totals)) : 0;
  const prf = PROFILES[domIdx] || PROFILES[0];
  const radar = DIMS.map((l, i) => ({ dim: l, v: norm[i] || 0 }));

  const wrap = {
    fontFamily: "'Lora', Georgia, serif",
    background: G.bg, color: G.text,
    minHeight: '100vh', display: 'flex',
    justifyContent: 'center', padding: '0 18px',
  };
  const inner = {
    width: '100%', maxWidth: '520px',
    padding: '44px 0 64px',
    opacity: vis ? 1 : 0,
    transition: 'opacity 0.22s ease',
  };
  const TAG = { fontFamily: "'Cormorant Garamond', serif", fontSize: '10px', letterSpacing: '0.22em', color: G.gold, textTransform: 'uppercase', marginBottom: '14px', display: 'block' };
  const H1 = { fontFamily: "'Cormorant Garamond', serif", fontSize: 'clamp(2.1rem, 6vw, 2.9rem)', fontWeight: 600, lineHeight: 1.15, marginBottom: '14px' };
  const LEAD = { fontSize: '0.88rem', lineHeight: 1.85, color: G.dim, marginBottom: '28px' };
  const HR = { width: '36px', height: '1px', background: G.gold, margin: '18px 0', opacity: 0.45 };

  // INTRO
  if (phase === "intro") return (
    <div style={wrap}>
      <style>{FONTS}</style>
      <div style={inner}>
        <span style={TAG}>Diagnóstico literario</span>
        <h1 style={H1}>Mapa de<br />tu Voz</h1>
        <div style={HR} />
        <p style={LEAD}>
          Cinco preguntas. Sin respuestas correctas.<br />
          Al final: un mapa de los rasgos que ya viven en ti<br />
          y que todavía no aprendes del todo a confiar.
        </p>
        <p style={{ fontSize: '0.8rem', color: G.dim, marginBottom: '26px', fontStyle: 'italic' }}>
          Desarrollado para escritores que trabajan en múltiples formatos.
        </p>
        <button style={btnPrimary(true)} onClick={() => go(() => setPhase("q"))}>
          Comenzar →
        </button>
      </div>
    </div>
  );

  // QUESTION
  if (phase === "q") {
    const Q = QUESTIONS[qIdx];
    return (
      <div style={wrap}>
        <style>{FONTS}</style>
        <div style={inner}>
          {/* progress */}
          <div style={{ display: 'flex', gap: '5px', marginBottom: '34px' }}>
            {QUESTIONS.map((_, i) => (
              <div key={i} style={{
                flex: 1, height: '2px', borderRadius: '1px',
                background: i < qIdx ? G.gold : i === qIdx ? G.goldLight : G.border,
                transition: 'background 0.3s',
              }} />
            ))}
          </div>
          <span style={TAG}>{qIdx + 1} de {QUESTIONS.length}</span>
          <div style={{ fontFamily: "'Cormorant Garamond', serif", fontSize: 'clamp(1.25rem, 4vw, 1.55rem)', fontWeight: 600, lineHeight: 1.35, marginBottom: '6px' }}>
            {Q.q}
          </div>
          <div style={{ fontSize: '0.8rem', color: G.dim, fontStyle: 'italic', marginBottom: '22px' }}>
            {Q.hint}
          </div>
          <div>
            {Q.opts.map((opt, i) => (
              <div key={i} style={optStyle(sel === opt)} onClick={() => setSel(opt)}>
                {opt.t}
              </div>
            ))}
          </div>
          <button style={btnPrimary(!!sel)} onClick={next} disabled={!sel}>
            {qIdx + 1 < QUESTIONS.length ? 'Siguiente →' : 'Ver mi mapa →'}
          </button>
        </div>
      </div>
    );
  }

  // RESULTS
  if (phase === "results") return (
    <div style={wrap}>
      <style>{FONTS}</style>
      <div style={inner}>
        <span style={TAG}>Tu perfil de voz</span>
        <div style={{ fontFamily: "'Cormorant Garamond', serif", fontSize: 'clamp(1.5rem, 4.5vw, 2.1rem)', fontWeight: 600, lineHeight: 1.25, marginBottom: '6px', display: 'flex', alignItems: 'center', gap: '10px', flexWrap: 'wrap' }}>
          <span style={{ color: prf.color }}>{prf.icon}</span>
          <span>{prf.name}</span>
        </div>
        <div style={HR} />

        {/* Radar */}
        <div style={{ width: '100%', height: 210, marginBottom: '8px' }}>
          <ResponsiveContainer width="100%" height="100%">
            <RadarChart data={radar} margin={{ top: 8, right: 28, bottom: 8, left: 28 }}>
              <PolarGrid stroke={G.border} />
              <PolarAngleAxis
                dataKey="dim"
                tick={{ fill: G.dim, fontSize: 10.5, fontFamily: 'Lora, serif' }}
              />
              <Radar dataKey="v" stroke={prf.color} fill={prf.color} fillOpacity={0.12} strokeWidth={1.5} />
            </RadarChart>
          </ResponsiveContainer>
        </div>

        {/* Score bars */}
        <div style={{ marginBottom: '20px' }}>
          {DIMS.map((l, i) => (
            <div key={i} style={{ marginBottom: '8px' }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: '0.74rem', color: G.dim, marginBottom: '3px' }}>
                <span>{l}</span>
                <span style={{ color: i === domIdx ? prf.color : G.dim }}>{norm[i]}%</span>
              </div>
              <div style={{ height: '2px', background: G.border, borderRadius: '1px' }}>
                <div style={{ height: '100%', width: `${norm[i]}%`, background: i === domIdx ? prf.color : G.accent, borderRadius: '1px', transition: 'width 1s ease' }} />
              </div>
            </div>
          ))}
        </div>

        {/* Description */}
        <div style={{ background: G.card, border: `1px solid ${G.border}`, borderRadius: '4px', padding: '20px', marginBottom: '24px' }}>
          <p style={{ fontSize: '0.88rem', lineHeight: 1.82, color: G.text }}>{prf.desc}</p>
        </div>

        {/* Exercises */}
        <span style={{ ...TAG, marginBottom: '16px' }}>3 ejercicios para tu voz</span>
        {prf.exs.map((ex, i) => (
          <div key={i} style={{ borderLeft: `2px solid ${prf.color}`, paddingLeft: '16px', marginBottom: '20px' }}>
            <div style={{ fontSize: '0.78rem', fontWeight: 600, color: prf.color, marginBottom: '5px', letterSpacing: '0.04em', textTransform: 'uppercase' }}>
              {ex.title}
            </div>
            <div style={{ fontSize: '0.85rem', lineHeight: 1.75, color: G.dim }}>{ex.body}</div>
          </div>
        ))}

        <div style={{ marginTop: '28px', paddingTop: '20px', borderTop: `1px solid ${G.border}` }}>
          <p style={{ fontSize: '0.78rem', color: G.dim, fontStyle: 'italic', marginBottom: '14px' }}>
            Recuerda: la voz no se construye. Se descubre. Estos ejercicios son pistas, no fórmulas.
          </p>
          <button
            style={{ background: 'transparent', color: G.dim, border: `1px solid ${G.border}`, padding: '10px 20px', fontFamily: "'Lora', serif", fontSize: '0.82rem', cursor: 'pointer', borderRadius: '2px', letterSpacing: '0.04em', transition: 'all 0.18s' }}
            onClick={restart}
          >
            ← Repetir diagnóstico
          </button>
        </div>
      </div>
    </div>
  );
}
