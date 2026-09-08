import { Component, type ErrorInfo, type ReactNode } from 'react'
import PageState from './PageState'

interface PageErrorBoundaryProps {
  children: ReactNode
  onReload: () => void
}

interface PageErrorBoundaryState {
  error: Error | null
  preloadFailed: boolean
}

export default class PageErrorBoundary extends Component<
  PageErrorBoundaryProps,
  PageErrorBoundaryState
> {
  state: PageErrorBoundaryState = { error: null, preloadFailed: false }

  static getDerivedStateFromError(error: Error) {
    return { error }
  }

  componentDidCatch(error: Error, information: ErrorInfo) {
    console.error('Momento page failed to render', error, information.componentStack)
  }

  private handlePreloadError = () => {
    // Do not preventDefault: the rejected import must still reach React's error boundary.
    this.setState({ preloadFailed: true })
  }

  componentDidMount() {
    window.addEventListener('vite:preloadError', this.handlePreloadError)
  }

  componentWillUnmount() {
    window.removeEventListener('vite:preloadError', this.handlePreloadError)
  }

  render() {
    const { error, preloadFailed } = this.state
    if (!error && !preloadFailed) return this.props.children

    const moduleFailed =
      preloadFailed ||
      /Failed to fetch dynamically imported module|Importing a module script failed|error loading dynamically imported module|Unable to preload CSS|Loading chunk .+ failed/i.test(
        error?.message ?? ''
      )

    return (
      <main role="alert" className="mx-auto max-w-2xl p-6">
        <PageState
          icon={<span aria-hidden="true">!</span>}
          title={moduleFailed ? 'Page files could not be loaded' : 'This page encountered an error'}
          description={
            moduleFailed
              ? 'Momento may have been updated while this tab was open, or the network is unavailable. Refresh to load the current version. Unsaved changes may be lost.'
              : 'Refresh to try again. If this continues, check the browser console for details. Unsaved changes may be lost.'
          }
          action={
            <button
              className="rounded-lg bg-primary px-4 py-2 font-medium text-primary-foreground focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
              type="button"
              onClick={this.props.onReload}
            >
              Refresh page
            </button>
          }
        />
      </main>
    )
  }
}
