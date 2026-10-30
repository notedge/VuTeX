export default {
    delivery: {
        default: "static",
        profiles: {
            static: {
                host: "browser",
                assembly: "web-static",
                name: "cdn",
            },
        },
    },
};
