import asyncio
import logging

from lattice_ai.config import Settings
from lattice_ai.server.app import serve


def main() -> None:
    settings = Settings()
    logging.basicConfig(
        level=settings.log_level.upper(),
        format="%(asctime)s %(levelname)s %(name)s %(message)s",
    )
    asyncio.run(serve(settings))


if __name__ == "__main__":
    main()
