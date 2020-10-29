require('esbuild')
    .build({
        entryPoints: ["./src/bind.ts"],
        outfile: './src/katex.min.js',
        format: 'esm',
        minify: true,
        bundle: true,
    })
    .catch(() => process.exit(1))
