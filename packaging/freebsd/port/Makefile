PORTNAME=	big-code-analysis
DISTVERSION=	@@VERSION@@
CATEGORIES=	devel
MASTER_SITES=	https://github.com/dekobon/big-code-analysis/releases/download/v${DISTVERSION}/
DISTNAME=	big-code-analysis-${DISTVERSION}-x86_64-unknown-freebsd
EXTRACT_SUFX=	.tar.gz

MAINTAINER=	elijah@zupancic.name
COMMENT=	Compute maintainability metrics for source code in many languages
WWW=		https://github.com/dekobon/big-code-analysis

LICENSE=	MPL20
LICENSE_FILE=	${WRKSRC}/LICENSE

NO_BUILD=	yes
ONLY_FOR_ARCHS=	amd64

PLIST_FILES=	bin/bca \
		bin/bca-web \
		share/licenses/big-code-analysis/THIRD-PARTY-LICENSES-bca.md \
		share/licenses/big-code-analysis/THIRD-PARTY-LICENSES-bca-web.md

do-install:
	${INSTALL_PROGRAM} ${WRKSRC}/bca     ${STAGEDIR}${PREFIX}/bin/bca
	${INSTALL_PROGRAM} ${WRKSRC}/bca-web ${STAGEDIR}${PREFIX}/bin/bca-web
	${MKDIR} ${STAGEDIR}${PREFIX}/share/licenses/big-code-analysis
	${INSTALL_DATA} ${WRKSRC}/THIRD-PARTY-LICENSES-bca.md \
		${STAGEDIR}${PREFIX}/share/licenses/big-code-analysis/THIRD-PARTY-LICENSES-bca.md
	${INSTALL_DATA} ${WRKSRC}/THIRD-PARTY-LICENSES-bca-web.md \
		${STAGEDIR}${PREFIX}/share/licenses/big-code-analysis/THIRD-PARTY-LICENSES-bca-web.md

.include <bsd.port.mk>
