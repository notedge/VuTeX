export interface Specimen {
    id: string;
    name: string;
    category: string;
    tex: string;
    note: string;
}

/** Curated corpus: every entry is display-mode TeX, ordered from plain to stress. */
export const SPECIMENS: Specimen[] = [
    {
        id: "euler",
        name: "欧拉恒等式",
        category: "经典",
        tex: String.raw`e^{i\pi} + 1 = 0`,
        note: "最小字号下的基线对齐是最容易暴露排版问题的位置。",
    },
    {
        id: "gauss",
        name: "高斯积分",
        category: "经典",
        tex: String.raw`\int_{-\infty}^{\infty} e^{-x^2}\,\mathrm{d}x = \sqrt{\pi}`,
        note: "积分号与上下限的间距来自字体度量，而不是固定像素。",
    },
    {
        id: "quadratic",
        name: "求根公式",
        category: "分式",
        tex: String.raw`x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}`,
        note: "分式线厚度、根式连接角与嵌套深度。",
    },
    {
        id: "inverse",
        name: "矩阵求逆",
        category: "结构",
        tex: String.raw`\begin{pmatrix} a & b \\ c & d \end{pmatrix}^{-1} = \frac{1}{ad-bc} \begin{pmatrix} d & -b \\ -c & a \end{pmatrix}`,
        note: "括号随内容生长，行距由矩阵结构而非外层容器决定。",
    },
    {
        id: "sum",
        name: "平方和",
        category: "结构",
        tex: String.raw`\sum_{k=1}^{n} k^2 = \frac{n(n+1)(2n+1)}{6}`,
        note: "上下限的垂直位置决定整行行高。",
    },
    {
        id: "limit",
        name: "自然常数",
        category: "结构",
        tex: String.raw`\lim_{n \to \infty} \left(1 + \frac{1}{n}\right)^{n} = e`,
        note: "自适应括号与上下标指数同时出现。",
    },
    {
        id: "cauchy",
        name: "柯西–施瓦茨",
        category: "经典",
        tex: String.raw`\left(\sum_{i=1}^{n} a_i b_i\right)^{2} \le \left(\sum_{i=1}^{n} a_i^{2}\right)\left(\sum_{i=1}^{n} b_i^{2}\right)`,
        note: "长行能否在同一基线上稳定延伸。",
    },
    {
        id: "cases",
        name: "分段函数",
        category: "结构",
        tex: String.raw`f(x) = \begin{cases} x^{2} & x \ge 0 \\ -x & x < 0 \end{cases}`,
        note: "左括号与多行分支的对齐。",
    },
    {
        id: "continued",
        name: "连分数",
        category: "分式",
        tex: String.raw`\cfrac{1}{\sqrt{2 + \cfrac{1}{\sqrt{2 + \cfrac{1}{\sqrt{2 + \cdots}}}}}}`,
        note: "递归嵌套的极限情形，分式缩放策略各不相同。",
    },
    {
        id: "aligned",
        name: "麦克斯韦方程组",
        category: "对齐",
        tex: String.raw`\begin{aligned} \nabla \cdot \mathbf{E} &= \frac{\rho}{\varepsilon_{0}} & \nabla \cdot \mathbf{B} &= 0 \\ \nabla \times \mathbf{E} &= -\frac{\partial \mathbf{B}}{\partial t} & \nabla \times \mathbf{B} &= \mu_{0}\mathbf{J} + \mu_{0}\varepsilon_{0}\frac{\partial \mathbf{E}}{\partial t} \end{aligned}`,
        note: "多列对齐是最检验引擎表格模型的一项。",
    },
    {
        id: "accents",
        name: "堆叠标注",
        category: "标注",
        tex: String.raw`\overset{\frown}{AB} \quad \underbrace{a + b + \cdots + z}_{26}`,
        note: "上下堆叠物的宽度参与整体度量。",
    },
    {
        id: "faces",
        name: "数学字体",
        category: "字形",
        tex: String.raw`\mathbb{R}\mathbb{C}\mathbb{N}\ \mathcal{L}\ \mathfrak{g}\ \mathsf{XYZ}\ \mathtt{code}`,
        note: "字母表覆盖度与字体回退策略。",
    },
    {
        id: "nested",
        name: "根式嵌套",
        category: "分式",
        tex: String.raw`\sqrt[3]{\frac{\alpha}{1 + \frac{\beta}{\gamma}}}`,
        note: "根指数与深分式的组合压缩。",
    },
    {
        id: "series",
        name: "泰勒展开",
        category: "经典",
        tex: String.raw`\sin x = \sum_{n=0}^{\infty} \frac{(-1)^{n}}{(2n+1)!}\,x^{2n+1}`,
        note: "密集上下标与分式的统一基线。",
    },
];

export const DEFAULT_TEX = SPECIMENS[7].tex;
